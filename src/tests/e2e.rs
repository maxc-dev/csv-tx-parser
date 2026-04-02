use std::collections::HashMap;
use std::env::temp_dir;
use std::fs::File;
use std::io::Write;
use std::process::Command;

fn run_engine_from_path(path: &std::path::Path) -> (String, String, i32) {
    let output = Command::new("cargo")
        .args(["run", "--", path.to_str().unwrap()])
        .output()
        .expect("Failed to execute process");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

fn write_csv(name: &str, content: &str) -> std::path::PathBuf {
    let path = temp_dir().join(format!("e2e_{name}.csv"));
    let mut f = File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

fn run_engine(csv_content: &str, name: &str) -> (String, String, i32) {
    let path = write_csv(name, csv_content);
    let output = Command::new("cargo")
        .args(["run", "--", path.to_str().unwrap()])
        .output()
        .expect("Failed to execute process");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Helper: parse CSV output into a map of client_id -> (available, held, total, locked)
fn parse_output(stdout: &str) -> HashMap<u16, (String, String, String, String)> {
    let mut map = HashMap::new();
    for line in stdout.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() == 5 {
            let client: u16 = cols[0].trim().parse().unwrap();
            map.insert(
                client,
                (
                    cols[1].trim().to_string(),
                    cols[2].trim().to_string(),
                    cols[3].trim().to_string(),
                    cols[4].trim().to_string(),
                ),
            );
        }
    }
    map
}

#[test]
fn e2e_basic_deposits_and_withdrawal() {
    let csv = "type, client, tx, amount\n\
               deposit, 1, 1, 1.0\n\
               deposit, 2, 2, 2.0\n\
               deposit, 1, 3, 2.0\n\
               withdrawal, 1, 4, 1.5\n\
               withdrawal, 2, 5, 3.0\n";
    let (stdout, _, code) = run_engine(csv, "basic");
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].0, "1.5000"); // available
    assert_eq!(accounts[&1].2, "1.5000"); // total
    assert_eq!(accounts[&2].0, "2.0000"); // withdrawal failed (insufficient)
    assert_eq!(accounts[&2].3, "false"); // not locked
}

#[test]
fn e2e_dispute_resolve_flow() {
    let csv = "type,client,tx,amount\n\
               deposit,1,1,10.0\n\
               dispute,1,1,\n\
               resolve,1,1,\n";
    let (stdout, _, _) = run_engine(csv, "dispute_resolve");
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].0, "10.0000"); // available restored
    assert_eq!(accounts[&1].1, "0.0000"); // held cleared
    assert_eq!(accounts[&1].3, "false");
}

#[test]
fn e2e_dispute_chargeback_freezes_account() {
    let csv = "type,client,tx,amount\n\
               deposit,1,1,10.0\n\
               dispute,1,1,\n\
               chargeback,1,1,\n";
    let (stdout, _, _) = run_engine(csv, "chargeback");
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].0, "0.0000"); // available
    assert_eq!(accounts[&1].1, "0.0000"); // held
    assert_eq!(accounts[&1].2, "0.0000"); // total
    assert_eq!(accounts[&1].3, "true"); // locked
}

#[test]
fn e2e_frozen_account_rejects_subsequent_deposits() {
    let csv = "type,client,tx,amount\n\
               deposit,1,1,10.0\n\
               dispute,1,1,\n\
               chargeback,1,1,\n\
               deposit,1,2,50.0\n";
    let (stdout, _, _) = run_engine(csv, "frozen_reject");
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].2, "0.0000"); // total still 0
    assert_eq!(accounts[&1].3, "true");
}

#[test]
fn e2e_multi_client_isolation() {
    let csv = "type,client,tx,amount\n\
               deposit,1,1,100.0\n\
               deposit,2,2,200.0\n\
               dispute,1,1,\n\
               chargeback,1,1,\n\
               deposit,2,3,50.0\n";
    let (stdout, _, _) = run_engine(csv, "isolation");
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].3, "true"); // client 1 frozen
    assert_eq!(accounts[&2].0, "250.0000"); // client 2 unaffected
    assert_eq!(accounts[&2].3, "false");
}

#[test]
fn e2e_whitespace_handling() {
    let csv = "type , client , tx , amount\n\
               deposit , 1 , 1 , 5.5\n";
    let (stdout, _, code) = run_engine(csv, "whitespace");
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].0, "5.5000");
}

#[test]
fn e2e_decimal_precision() {
    let csv = "type,client,tx,amount\n\
               deposit,1,1,1.2345\n\
               deposit,1,2,0.0001\n";
    let (stdout, _, _) = run_engine(csv, "precision");
    let accounts = parse_output(&stdout);
    assert_eq!(accounts[&1].0, "1.2346"); // 1.2345 + 0.0001
}

#[test]
fn e2e_no_args_exits_with_error() {
    let output = Command::new("cargo")
        .args(["run", "--"])
        .output()
        .expect("Failed to execute");
    assert_ne!(output.status.code().unwrap(), 0);
}

#[test]
fn e2e_output_has_correct_header() {
    let csv = "type,client,tx,amount\ndeposit,1,1,1.0\n";
    let (stdout, _, _) = run_engine(csv, "header");
    let header = stdout.lines().next().unwrap();
    assert_eq!(header, "client,available,held,total,locked");
}

/// 10,000 deposits across 100 clients — verifies streaming handles large files
/// and balances accumulate correctly.
#[test]
fn e2e_large_10k_deposits_100_clients() {
    let path = temp_dir().join("e2e_large_10k.csv");
    {
        let mut f = File::create(&path).unwrap();
        writeln!(f, "type,client,tx,amount").unwrap();
        // 100 clients, 100 deposits each = 10,000 rows
        // Each client gets deposits of 1.0000 x 100 = 100.0000
        let mut tx_id: u32 = 1;
        for client in 1..=100u16 {
            for _ in 0..100 {
                writeln!(f, "deposit,{},{},1.0", client, tx_id).unwrap();
                tx_id += 1;
            }
        }
    }
    let (stdout, _, code) = run_engine_from_path(&path);
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts.len(), 100);
    for client in 1..=100u16 {
        let acct = &accounts[&client];
        assert_eq!(acct.0, "100.0000", "client {} available", client);
        assert_eq!(acct.1, "0.0000", "client {} held", client);
        assert_eq!(acct.2, "100.0000", "client {} total", client);
        assert_eq!(acct.3, "false", "client {} locked", client);
    }
}

/// 50,000 mixed transactions: deposits, withdrawals, disputes, resolves, chargebacks
/// across 200 clients. Verifies correctness at scale with all transaction types.
#[test]
fn e2e_large_50k_mixed_transactions() {
    let path = temp_dir().join("e2e_large_50k.csv");
    {
        let mut f = File::create(&path).unwrap();
        writeln!(f, "type,client,tx,amount").unwrap();
        let mut tx_id: u32 = 1;

        for client in 1..=200u16 {
            // Phase 1: 200 deposits of 5.0 each = 1000.0 per client
            let first_deposit_tx = tx_id;
            for _ in 0..200 {
                writeln!(f, "deposit,{},{},5.0", client, tx_id).unwrap();
                tx_id += 1;
            }

            // Phase 2: 40 withdrawals of 10.0 each = 400.0 withdrawn
            // Available after: 600.0
            for _ in 0..40 {
                writeln!(f, "withdrawal,{},{},10.0", client, tx_id).unwrap();
                tx_id += 1;
            }

            // Phase 3: 5 failed withdrawals (insufficient funds — requesting 700.0 each)
            for _ in 0..5 {
                writeln!(f, "withdrawal,{},{},700.0", client, tx_id).unwrap();
                tx_id += 1;
            }

            // Phase 4: dispute + resolve on first deposit (5.0)
            // available: 600 -> 595 -> 600, held: 0 -> 5 -> 0
            writeln!(f, "dispute,{},{},", client, first_deposit_tx).unwrap();
            writeln!(f, "resolve,{},{},", client, first_deposit_tx).unwrap();
        }
    }
    let (stdout, _, code) = run_engine_from_path(&path);
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts.len(), 200);
    for client in 1..=200u16 {
        let acct = &accounts[&client];
        assert_eq!(acct.0, "600.0000", "client {} available", client);
        assert_eq!(acct.1, "0.0000", "client {} held", client);
        assert_eq!(acct.2, "600.0000", "client {} total", client);
        assert_eq!(acct.3, "false", "client {} locked", client);
    }
}

/// 500 clients where every even client gets chargebacked and frozen.
/// Odd clients remain active with correct balances.
/// ~5,000 transactions total.
#[test]
fn e2e_large_mass_chargebacks() {
    let path = temp_dir().join("e2e_large_chargebacks.csv");
    {
        let mut f = File::create(&path).unwrap();
        writeln!(f, "type,client,tx,amount").unwrap();
        let mut tx_id: u32 = 1;

        for client in 1..=500u16 {
            let deposit_tx = tx_id;
            // Deposit 50.0
            writeln!(f, "deposit,{},{},50.0", client, tx_id).unwrap();
            tx_id += 1;

            // Second deposit 25.0
            writeln!(f, "deposit,{},{},25.0", client, tx_id).unwrap();
            tx_id += 1;

            // Withdraw 10.0
            writeln!(f, "withdrawal,{},{},10.0", client, tx_id).unwrap();
            tx_id += 1;

            if client % 2 == 0 {
                // Even clients: dispute first deposit, then chargeback → frozen
                writeln!(f, "dispute,{},{},", client, deposit_tx).unwrap();
                writeln!(f, "chargeback,{},{},", client, deposit_tx).unwrap();

                // Attempt another deposit after freeze — should be rejected
                writeln!(f, "deposit,{},{},100.0", client, tx_id).unwrap();
                tx_id += 1;
            }
        }
    }
    let (stdout, _, code) = run_engine_from_path(&path);
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts.len(), 500);

    for client in 1..=500u16 {
        let acct = &accounts[&client];
        if client % 2 == 0 {
            // Even: deposited 75, withdrew 10 = 65 available.
            // Then disputed 50 (available=15, held=50), chargebacked (held=0, total=15).
            // Frozen, so the 100.0 deposit is rejected.
            assert_eq!(acct.0, "15.0000", "client {} available", client);
            assert_eq!(acct.1, "0.0000", "client {} held", client);
            assert_eq!(acct.2, "15.0000", "client {} total", client);
            assert_eq!(acct.3, "true", "client {} locked", client);
        } else {
            // Odd: deposited 75, withdrew 10 = 65
            assert_eq!(acct.0, "65.0000", "client {} available", client);
            assert_eq!(acct.1, "0.0000", "client {} held", client);
            assert_eq!(acct.2, "65.0000", "client {} total", client);
            assert_eq!(acct.3, "false", "client {} locked", client);
        }
    }
}

/// 100,000 transactions with fractional amounts to stress-test decimal precision.
/// 1000 clients, each gets 100 deposits of 0.0001 = 0.0100 per client.
#[test]
fn e2e_large_100k_precision_stress() {
    let path = temp_dir().join("e2e_large_100k_precision.csv");
    {
        let mut f = File::create(&path).unwrap();
        writeln!(f, "type,client,tx,amount").unwrap();
        let mut tx_id: u32 = 1;
        for client in 1..=1000u16 {
            for _ in 0..100 {
                writeln!(f, "deposit,{},{},0.0001", client, tx_id).unwrap();
                tx_id += 1;
            }
        }
    }
    let (stdout, _, code) = run_engine_from_path(&path);
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts.len(), 1000);
    for client in 1..=1000u16 {
        let acct = &accounts[&client];
        assert_eq!(acct.0, "0.0100", "client {} available", client);
        assert_eq!(acct.2, "0.0100", "client {} total", client);
    }
}

/// Complex lifecycle: 50 clients each go through deposit → dispute → resolve → deposit →
/// dispute → chargeback → rejected deposit. Tests full state machine at scale.
#[test]
fn e2e_large_full_lifecycle_50_clients() {
    let path = temp_dir().join("e2e_large_lifecycle.csv");
    {
        let mut f = File::create(&path).unwrap();
        writeln!(f, "type,client,tx,amount").unwrap();
        let mut tx_id: u32 = 1;

        for client in 1..=50u16 {
            // Deposit 100.0 (tx A)
            let tx_a = tx_id;
            writeln!(f, "deposit,{},{},100.0", client, tx_id).unwrap();
            tx_id += 1;

            // Deposit 200.0 (tx B)
            let tx_b = tx_id;
            writeln!(f, "deposit,{},{},200.0", client, tx_id).unwrap();
            tx_id += 1;

            // Withdraw 50.0
            writeln!(f, "withdrawal,{},{},50.0", client, tx_id).unwrap();
            tx_id += 1;
            // State: available=250, held=0, total=250

            // Dispute tx A (100.0) → available=150, held=100, total=250
            writeln!(f, "dispute,{},{},", client, tx_a).unwrap();

            // Resolve tx A → available=250, held=0, total=250
            writeln!(f, "resolve,{},{},", client, tx_a).unwrap();

            // Deposit 50.0 (tx C)
            writeln!(f, "deposit,{},{},50.0", client, tx_id).unwrap();
            tx_id += 1;
            // State: available=300, held=0, total=300

            // Dispute tx B (200.0) → available=100, held=200, total=300
            writeln!(f, "dispute,{},{},", client, tx_b).unwrap();

            // Chargeback tx B → available=100, held=0, total=100, FROZEN
            writeln!(f, "chargeback,{},{},", client, tx_b).unwrap();

            // Rejected deposit (account frozen)
            writeln!(f, "deposit,{},{},999.0", client, tx_id).unwrap();
            tx_id += 1;
        }
    }
    let (stdout, _, code) = run_engine_from_path(&path);
    assert_eq!(code, 0);
    let accounts = parse_output(&stdout);
    assert_eq!(accounts.len(), 50);
    for client in 1..=50u16 {
        let acct = &accounts[&client];
        assert_eq!(acct.0, "100.0000", "client {} available", client);
        assert_eq!(acct.1, "0.0000", "client {} held", client);
        assert_eq!(acct.2, "100.0000", "client {} total", client);
        assert_eq!(acct.3, "true", "client {} locked", client);
    }
}
