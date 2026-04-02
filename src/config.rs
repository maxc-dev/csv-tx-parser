pub struct TransactionProcessorConfig {
    pub fail_file_on_error: bool,
}

impl Default for TransactionProcessorConfig {
    fn default() -> Self {
        Self {
            fail_file_on_error: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_fail_file_on_error_is_false() {
        let config = TransactionProcessorConfig::default();
        assert!(!config.fail_file_on_error, "Default config should not fail on error");
    }
}