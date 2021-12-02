use super::{create_default_buffer, install_tracing_helpers, with_temp_dir, SizedRecord};

#[tokio::test]
async fn basic_read_write_loop() {
    let _ = install_tracing_helpers();
    with_temp_dir(|dir| {
        let data_dir = dir.to_path_buf();

        async move {
            // Create a regular buffer, no customizations required.
            let (mut writer, mut reader, acker, ledger) = create_default_buffer(data_dir).await;

            assert_eq!(ledger.state().get_total_records(), 0);
            assert_eq!(ledger.state().get_total_buffer_size(), 0);

            let expected_items = (512..768)
                .into_iter()
                .cycle()
                .take(2000)
                .map(|i| SizedRecord(i))
                .collect::<Vec<_>>();
            let input_items = expected_items.clone();

            // Now create a reader and writer task that will take a set of input messages, buffer
            // them, read them out, and then make sure nothing was missed.
            let write_task = tokio::spawn(async move {
                for item in input_items {
                    writer
                        .write_record(item)
                        .await
                        .expect("write should not fail");
                }
                writer.flush().await.expect("writer flush should not fail");
                writer.close();
            });

            let read_task = tokio::spawn(async move {
                let mut items = Vec::new();
                while let Some(record) = reader.next().await.expect("reader should not fail") {
                    items.push(record);
                    acker.acknowledge_records(1);
                }
                items
            });

            // Wait for both tasks to complete.
            write_task.await.expect("write task should not panic");
            let actual_items = read_task.await.expect("read task should not panic");

            // All records should be consumed at this point.
            assert_eq!(ledger.state().get_total_records(), 0);
            assert_eq!(ledger.state().get_total_buffer_size(), 0);

            // Make sure we got the right items.
            assert_eq!(actual_items, expected_items);
        }
    })
    .await
}
