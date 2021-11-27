use std::{
    fmt, io,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    time::Instant,
};

use bytecheck::CheckBytes;
use bytes::BytesMut;
use crossbeam_utils::atomic::AtomicCell;
use fslock::LockFile;
use memmap2::{MmapMut, MmapOptions};
use rkyv::{with::Atomic, Archive, Serialize};
use snafu::{ResultExt, Snafu};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::Notify};

use super::{backed_archive::BackedArchive, common::DiskBufferConfig, ser::SerializeError};

#[derive(Debug, Snafu)]
pub enum LedgerLoadCreateError {
    #[snafu(display("ledger I/O error: {}", source))]
    Io { source: io::Error },
    #[snafu(display(
        "failed to lock buffer.lock; is another Vector process running and using this buffer?"
    ))]
    LedgerLockAlreadyHeld,
    #[snafu(display("failed to deserialize ledger from buffer: {}", reason))]
    FailedToDeserialize { reason: String },
    #[snafu(display("failed to serialize ledger to buffer: {}", reason))]
    FailedToSerialize { reason: String },
}

/// Ledger state.
///
/// Stores the relevant information related to both the reader and writer.  Gets serailized and
/// stored on disk, and is managed via a memory-mapped file.
///
/// # Warning
///
/// - Do not add fields to this struct.
/// - Do not remove fields from this struct.
/// - Do not change the type of fields in this struct.
/// - Do not change the order of fields this struct.
///
/// Doing so will change the serialized representation.  This will break things.
///
/// Do not do any of the listed things unless you _absolutely_ know what you're doing. :)
#[derive(Archive, Serialize, Debug)]
#[archive_attr(derive(CheckBytes, Debug))]
pub struct LedgerState {
    /// Total number of records persisted in this buffer.
    #[with(Atomic)]
    total_records: AtomicU64,
    /// Total size of all data files used by this buffer.
    #[with(Atomic)]
    total_buffer_size: AtomicU64,
    /// Next record ID to use when writing a record.
    #[with(Atomic)]
    writer_next_record_id: AtomicU64,
    /// The current data file ID being written to.
    #[with(Atomic)]
    writer_current_data_file_id: AtomicU16,
    /// The current data file ID being read from.
    #[with(Atomic)]
    reader_current_data_file_id: AtomicU16,
    /// The last record ID read by the reader.
    #[with(Atomic)]
    reader_last_record_id: AtomicU64,
}

impl Default for LedgerState {
    fn default() -> Self {
        Self {
            total_records: AtomicU64::new(0),
            total_buffer_size: AtomicU64::new(0),
            // First record written is always 1, so that our defualt of 0 for
            // `reader_last_record_id` ensures we start up in a state of "alright, waiting to read
            // record #1 next".
            writer_next_record_id: AtomicU64::new(1),
            writer_current_data_file_id: AtomicU16::new(0),
            reader_current_data_file_id: AtomicU16::new(0),
            reader_last_record_id: AtomicU64::new(0),
        }
    }
}

impl ArchivedLedgerState {
    pub fn increment_records(&self, record_size: u64) {
        self.total_records.fetch_add(1, Ordering::AcqRel);
        self.total_buffer_size
            .fetch_add(record_size, Ordering::AcqRel);
    }

    pub fn decrement_records(&self, record_size: u64) {
        self.total_records.fetch_sub(1, Ordering::AcqRel);
        self.total_buffer_size
            .fetch_sub(record_size, Ordering::AcqRel);
    }

    pub fn get_total_buffer_size(&self) -> u64 {
        self.total_buffer_size.load(Ordering::Acquire)
    }

    pub fn get_total_records(&self) -> u64 {
        self.total_records.load(Ordering::Acquire)
    }

    pub fn decrement_total_buffer_size(&self, amount: u64) {
        self.total_buffer_size.fetch_sub(amount, Ordering::AcqRel);
    }

    pub fn decrement_total_records(&self, amount: u64) {
        self.total_records.fetch_sub(amount, Ordering::AcqRel);
    }

    pub fn get_next_writer_record_id(&self) -> u64 {
        self.writer_next_record_id.load(Ordering::Acquire)
    }

    pub fn increment_next_writer_record_id(&self) {
        self.writer_next_record_id.fetch_add(1, Ordering::AcqRel);
    }

    pub fn get_last_reader_record_id(&self) -> u64 {
        self.reader_last_record_id.load(Ordering::Acquire)
    }

    pub fn set_last_reader_record_id(&self, id: u64) {
        self.reader_last_record_id.store(id, Ordering::Release);
    }

    /// Gets the current writer file ID.
    pub fn get_current_writer_file_id(&self) -> u16 {
        self.writer_current_data_file_id.load(Ordering::Acquire)
    }

    /// Gets the next writer file ID.
    pub fn get_next_writer_file_id(&self) -> u16 {
        self.writer_current_data_file_id
            .load(Ordering::Acquire)
            .wrapping_add(1)
    }

    /// Increments the current writer file ID.
    pub fn increment_writer_file_id(&self) {
        self.writer_current_data_file_id
            .fetch_add(1, Ordering::AcqRel);
    }

    /// Gets the current reader file ID.
    pub fn get_current_reader_file_id(&self) -> u16 {
        self.reader_current_data_file_id.load(Ordering::Acquire)
    }

    /// Increments the current reader file ID.
    pub fn increment_reader_file_id(&self) {
        self.reader_current_data_file_id
            .fetch_add(1, Ordering::AcqRel);
    }
}

pub struct Ledger {
    // Buffer configuration.
    config: DiskBufferConfig,
    // Advisory lock for this buffer directory.
    ledger_lock: LockFile,
    // Ledger state.
    state: BackedArchive<MmapMut, LedgerState>,
    // Notifier for reader-related progress.
    reader_notify: Notify,
    // Notifier for writer-related progress.
    writer_notify: Notify,
    // Tracks when writer has fully shutdown.
    writer_done: AtomicBool,
    // Last flush of all unflushed files: ledger, data file, etc.
    last_flush: AtomicCell<Instant>,
}

impl Ledger {
    pub fn config(&self) -> &DiskBufferConfig {
        &self.config
    }

    pub fn state(&self) -> &ArchivedLedgerState {
        self.state.get_archive_ref()
    }

    pub fn get_current_reader_data_file_path(&self) -> PathBuf {
        self.get_data_file_path(self.state().get_current_reader_file_id())
    }

    pub fn get_current_writer_data_file_path(&self) -> PathBuf {
        self.get_data_file_path(self.state().get_current_writer_file_id())
    }

    pub fn get_next_writer_data_file_path(&self) -> PathBuf {
        self.get_data_file_path(self.state().get_next_writer_file_id())
    }

    pub fn get_data_file_path(&self, file_id: u16) -> PathBuf {
        self.config
            .data_dir
            .join(format!("buffer-data-{}.dat", file_id))
    }

    /// Waits for a signal from the reader that an entire data file has been read and subsequently deleted.
    #[instrument(skip(self), level = "trace")]
    pub async fn wait_for_reader(&self) {
        self.reader_notify.notified().await;
    }

    /// Waits for a signal from the writer that data has been written to a data file, or that a new
    /// data file has been created.
    #[instrument(skip(self), level = "trace")]
    pub async fn wait_for_writer(&self) {
        self.writer_notify.notified().await;
    }

    /// Notifies all tasks waiting on progress by the reader.
    #[instrument(skip(self), level = "trace")]
    pub fn notify_reader_waiters(&self) {
        self.reader_notify.notify_one();
    }

    /// Notifies all tasks waiting on progress by the writer.
    #[instrument(skip(self), level = "trace")]
    pub fn notify_writer_waiters(&self) {
        self.writer_notify.notify_one();
    }

    /// Determines whether or not all files should be flushed/fsync'd to disk.
    ///
    /// In the case of concurrent callers when the flush deadline has been exceeded, only one caller
    /// will get a return value of `true`, and the others will receive `false`.  The caller that
    /// receives `true` is responsible for flushing the necessary files.
    pub fn should_flush(&self) -> bool {
        let last_flush = self.last_flush.load();
        if true || last_flush.elapsed() > self.config.flush_interval {
            if self
                .last_flush
                .compare_exchange(last_flush, Instant::now())
                .is_ok()
            {
                return true;
            }
        }

        false
    }

    pub fn track_write(&self, record_size: u64) {
        self.state().increment_records(record_size);
    }

    pub fn track_read(&self, record_size: u64) {
        self.state().decrement_records(record_size);
    }

    pub fn mark_writer_done(&self) -> bool {
        self.writer_done
            .compare_exchange_weak(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn is_writer_done(&self) -> bool {
        self.writer_done.load(Ordering::Acquire)
    }

    pub fn flush(&self) -> io::Result<()> {
        self.state.get_backing_ref().flush()
    }

    #[instrument(level = "trace")]
    pub async fn load_or_create(config: DiskBufferConfig) -> Result<Ledger, LedgerLoadCreateError> {
        // Acquire an exclusive lock on our lock file, which prevents another Vector process from
        // loading this buffer and clashing with us.  Specifically, though: this does _not_ prevent
        // another process from messing with our ledger files, or any of the data files, etc.
        let ledger_lock_path = config.data_dir.join("buffer.lock");
        let mut ledger_lock = LockFile::open(&ledger_lock_path).context(Io)?;
        if !ledger_lock.try_lock().context(Io)? {
            return Err(LedgerLoadCreateError::LedgerLockAlreadyHeld);
        }

        // Open the ledger file, which may involve creating it if it doesn't yet exist.
        let ledger_path = config.data_dir.join("buffer.db");
        let mut ledger_handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&ledger_path)
            .await
            .context(Io)?;

        // If we just created the ledger file, then we need to create the default ledger state, and
        // then serialize and write to the file, before trying to load it as a memory-mapped file.
        let ledger_metadata = ledger_handle.metadata().await.context(Io)?;
        let ledger_len = ledger_metadata.len();
        if ledger_len == 0 {
            let mut buf = BytesMut::new();
            loop {
                match BackedArchive::from_value(&mut buf, LedgerState::default()) {
                    Ok(archive) => {
                        let _ = ledger_handle
                            .write_all(archive.get_backing_ref())
                            .await
                            .context(Io)?;
                        break;
                    }
                    Err(SerializeError::FailedToSerialize(reason)) => {
                        return Err(LedgerLoadCreateError::FailedToSerialize { reason })
                    }
                    // Our buffer wasn't big enough, but that's OK!  Resize it and try again.
                    Err(SerializeError::BackingStoreTooSmall(_, min_len)) => buf.resize(min_len, 0),
                }
            }
        }

        // Load the ledger state by memory-mapping the ledger file, and zero-copy deserializing our
        // ledger state back out of it.
        let ledger_handle = ledger_handle.into_std().await;
        let ledger_mmap = unsafe { MmapOptions::new().map_mut(&ledger_handle).context(Io)? };

        let ledger_state = match BackedArchive::from_backing(ledger_mmap) {
            // Deserialized the ledger state without issue from an existing file.
            Ok(backed) => backed,
            // Either invalid data, or the buffer doesn't represent a valid ledger structure.
            Err(e) => {
                return Err(LedgerLoadCreateError::FailedToDeserialize {
                    reason: e.into_inner(),
                })
            }
        };

        Ok(Ledger {
            config,
            ledger_lock,
            state: ledger_state,
            reader_notify: Notify::new(),
            writer_notify: Notify::new(),
            writer_done: AtomicBool::new(false),
            last_flush: AtomicCell::new(Instant::now()),
        })
    }
}

impl fmt::Debug for Ledger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ledger")
            .field("config", &self.config)
            .field("ledger_lock", &self.ledger_lock)
            .field("state", &self.state.get_archive_ref())
            .field("reader_notify", &self.reader_notify)
            .field("writer_notify", &self.writer_notify)
            .field("writer_done", &self.writer_done.load(Ordering::Acquire))
            .field("last_flush", &self.last_flush)
            .finish()
    }
}
