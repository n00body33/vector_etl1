use metrics::counter;
use vector_core::internal_event::InternalEvent;

#[derive(Debug)]
pub struct ParserMatchError<'a> {
    pub value: &'a [u8],
}

impl InternalEvent for ParserMatchError<'_> {
    fn emit_logs(&self) {
        error!(
            message = "Pattern failed to match.",
            error = "Failed to match pattern",
            error_type = "condition_failed",
            stage = "processing",
            field = &super::truncate_string_at(&String::from_utf8_lossy(self.value), 60)[..],
            internal_log_rate_secs = 30
        );
    }

    fn emit_metrics(&self) {
        counter!(
            "component_errors_total", 1,
            "error" => "Failed to match pattern",
            "error_type" => "condition_failed",
            "stage" => "processing",
        );
        // deprecated
        counter!("processing_errors_total", 1, "error_type" => "failed_match");
    }
}

#[derive(Debug)]
pub struct ParserMissingFieldError<'a> {
    pub field: &'a str,
}

impl InternalEvent for ParserMissingFieldError<'_> {
    fn emit_logs(&self) {
        error!(
            message = "Field does not exist.",
            field = %self.field,
            error = "Field not found",
            error_type = "condition_failed",
            stage = "processing",
            internal_log_rate_secs = 10
        );
    }

    fn emit_metrics(&self) {
        counter!(
            "component_errors_total", 1,
            "error" => "Field not found",
            "error_type" => "condition_failed",
            "stage" => "processing",
            "field" => self.field.to_string(),
        );
        // deprecated
        counter!("processing_errors_total", 1, "error_type" => "missing_field");
    }
}

#[derive(Debug)]
pub struct ParserTargetExistsError<'a> {
    pub target_field: &'a str,
}

impl<'a> InternalEvent for ParserTargetExistsError<'a> {
    fn emit_logs(&self) {
        error!(
            message = "Target field already exists.",
            error = "Target field already exists",
            error_type = "condition_failed",
            stage = "processing",
            target_field = %self.target_field,
            internal_log_rate_secs = 30
        )
    }

    fn emit_metrics(&self) {
        counter!(
            "component_errors_total", 1,
            "error" => "Target field already exists",
            "error_type" => "condition_failed",
            "stage" => "processing",
            "target_field" => self.target_field.to_string(),
        );
        // deprecated
        counter!("processing_errors_total", 1, "error_type" => "target_field_exists");
    }
}

#[derive(Debug)]
pub struct ParserConversionError<'a> {
    pub name: &'a str,
    pub error: crate::types::Error,
}

impl<'a> InternalEvent for ParserConversionError<'a> {
    fn emit_logs(&self) {
        error!(
            message = "Could not convert types.",
            name = %self.name,
            error = ?self.error,
            error_type = "conversion_failed",
            stage = "processing",
            internal_log_rate_secs = 30
        );
    }

    fn emit_metrics(&self) {
        counter!(
            "component_errors_total", 1,
            "error" => self.error.to_string(),
            "error_type" => "conversion_failed",
            "stage" => "processing",
            "name" => self.name.to_string(),
        );
        // deprecated
        counter!("processing_errors_total", 1, "error_type" => "type_conversion_failed");
    }
}
