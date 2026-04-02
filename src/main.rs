use crate::config::TransactionProcessorConfig;
use crate::processor::{TransactionProcessor, TransactionProcessorError};
use std::io::stderr;
use std::env;
use std::process::ExitCode;

mod config;
mod io;
mod model;
mod processor;

fn run(file_path: &str) -> Result<(), TransactionProcessorError> {
    let config = TransactionProcessorConfig::default();
    let mut processor = TransactionProcessor::new(config.fail_file_on_error);
    processor.process_file(file_path)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.csv>", args[0]);
        return ExitCode::FAILURE;
    }
    tracing_subscriber::fmt().with_writer(stderr).init();
    match run(&args[1]) {
        Ok(_) => {
            tracing::info!("All transactions processed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!("Error processing file: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod main_tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn write_csv(name: &str, content: &str) -> std::path::PathBuf {
        let path = env::temp_dir().join(format!("main_test_{name}.csv"));
        let mut f = File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn run_with_valid_file_succeeds() {
        let csv = "type,client,tx,amount\ndeposit,1,1,10.0\n";
        let path = write_csv("valid", csv);
        let result = run(path.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_nonexistent_file_returns_error() {
        let result = run("nonexistent_file_xyz_12345.csv");
        assert!(result.is_err());
    }

    #[test]
    fn run_with_empty_csv_succeeds() {
        let csv = "type,client,tx,amount\n";
        let path = write_csv("empty", csv);
        let result = run(path.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_all_transaction_types_succeeds() {
        let csv = "type,client,tx,amount\n\
                   deposit,1,1,100.0\n\
                   withdrawal,1,2,10.0\n\
                   dispute,1,1,\n\
                   resolve,1,1,\n";
        let path = write_csv("all_types", csv);
        let result = run(path.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_malformed_rows_continues_in_default_mode() {
        let csv = "type,client,tx,amount\n\
                   deposit,1,1,10.0\n\
                   deposit,notanumber,2,5.0\n\
                   deposit,1,3,20.0\n";
        let path = write_csv("malformed", csv);
        let result = run(path.to_str().unwrap());
        assert!(result.is_ok());
    }

    // --- ExitCode tests (subprocess-based) ---

    fn run_binary(args: &[&str]) -> (String, String, i32) {
        let output = std::process::Command::new("cargo")
            .args(["run", "--"])
            .args(args)
            .output()
            .expect("Failed to execute process");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        let code = output.status.code().unwrap_or(-1);
        (stdout, stderr, code)
    }

    #[test]
    fn exit_code_success_on_valid_file() {
        let csv = "type,client,tx,amount\ndeposit,1,1,10.0\n";
        let path = write_csv("exit_success", csv);
        let (_, _, code) = run_binary(&[path.to_str().unwrap()]);
        assert_eq!(code, 0);
    }

    #[test]
    fn exit_code_failure_on_no_args() {
        let (_, _, code) = run_binary(&[]);
        assert_eq!(code, 1);
    }

    #[test]
    fn exit_code_failure_on_too_many_args() {
        let (_, _, code) = run_binary(&["file1.csv", "file2.csv"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn exit_code_failure_on_nonexistent_file() {
        let (_, _, code) = run_binary(&["nonexistent_file_exit_test.csv"]);
        assert_eq!(code, 1);
    }

    #[test]
    fn stderr_contains_usage_on_no_args() {
        let (_, stderr, _) = run_binary(&[]);
        assert!(stderr.contains("Usage:"));
    }
}
