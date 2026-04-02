### Payments Engine

A Rust CLI application that processes off-chain account transactions from a CSV file and outputs final client account states.

### Build & Run

Requires **Rust 2024 edition**, ensure your toolchain is up to date (`rustup update stable`).

```bash
cargo build
cargo run -- <input.csv>
```

Example:

```bash
cargo run -- transactions.csv
```

### Usage

Input CSV format:

```csv
type, client, tx, amount
deposit, 1, 1, 1.0
withdrawal, 1, 2, 0.5
dispute, 1, 1,
resolve, 1, 1,
chargeback, 1, 1,
```

Output CSV to stdout:

```csv
client,available,held,total,locked
1,0.5000,0.0000,0.5000,false
```

### Spec Compliance

| Requirement | Implementation |
|---|---|
| Single CLI arg (file path) | `main.rs` validates exactly one arg |
| Streaming CSV input | `csv::Reader` deserializes row-by-row, never loads full file |
| Whitespace tolerance | `Trim::All` on CSV reader |
| Deposit / Withdrawal | Increases/decreases `available` + `total`; insufficient funds → skip row |
| Dispute | Moves `available` → `held`; `total` unchanged |
| Resolve | Moves `held` → `available`; `total` unchanged |
| Chargeback | Decreases `held` + `total`; freezes account |
| Frozen accounts reject all transactions | Checked before any processing |
| Ignore invalid dispute/resolve/chargeback refs | Returns error, logged, row skipped |
| Output precision up to 4 decimal places | Fixed-point `u128` math with `SCALE = 10_000` |
| Client isolation | Accounts keyed by `ClientId`; client ID validated per transaction |
| Transaction state machine | `Pending → DepositComplete/WithdrawalComplete → Disputed → Resolved/Chargeback` |


### Design Decisions

| Decision                               | Discussion                                                                                                                                                                                                                                                                                             |
|----------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Numeric precision**                  | I went with fixed-point `i64`/`u128` scaled by `10_000` rather than pulling in `rust_decimal`. It's more manual and you lose arbitrary precision, but you get zero floating-point drift with minimal dependencies. I've learnt the hard way how important fixed-point values are in financial systems! |
| **Newtype wrappers**                   | `ClientId(u16)`, `TransactionId(u32)`, `Amount`, `Balance`  means the compiler catches you if you accidentally pass a client ID where a transaction ID is expected. Worth the verbosity for correctness.                                                                                               |
| **`NotSend` marker**                   | `Account` embeds `PhantomData<*const ()>` making it `!Send + !Sync`. This means you can't move accounts across threads if we ever go async, so it enforces single-threaded ownership at compile time and documents the intent that account mutation is not thread-safe.                                |
| **Only deposits disputable**           | Withdrawals reach `WithdrawalComplete` with no dispute transition. You can't dispute a withdrawal (the money already left), which matches real banking, you dispute incoming funds, not outgoing ones. Can always be made configurable if business rules change down the line.                         |
| **NonZeroU64 & Zero amounts rejected** | `Amount` wraps `NonZeroU64`, so you can't represent a zero-value transaction. A deposit or withdrawal of `0.0000` is meaningless, rejecting it at parse time is cleaner than sprinkling is i > 0 checks downstream!                                                                                    |
| **Configurable error handling**        | There's a `fail_file_on_error` flag: skip bad rows by default, or abort the entire file. Depends entirely on your requirements and how much you trust your upstream source... Mostly added this feature to show config-awareness without introducing too much scope creep.                             |
| **`Resolved` is terminal**             | Once a transaction is resolved, it can't be re-disputed. This is less flexible if business rules ever allow re-disputes, but it keeps the state machine simpler with fewer edge cases. Adding a `Resolved → Disputed` transition later would be straightforward if needed.                             |


### Testing

LLVM codecov: **98.68% (1342/1360 lines)**.

As mentioned below in the AI Disclaimer, I used Claude Open 4.6 to help create the unit tests to reach this level of coverage.

```bash
cargo test
```


### Future Scope

These are things that I considered during development but didn't implement yet to avoid scope creep and overengineering.

- **Concurrent transaction processing**: This would be very cool to implement if we ever needed to scale up. Would consider a MPSC bridge to the single-threaded processor. Happy to discuss this further.
- **Sequence ID**: A sequence ID could be used to ensure transactions are processed in order, even if they arrive out of order.
- **Transaction ID duplicate detection**: Currently tx IDs are tracked per-account. A global tx ID set would reject cross-account duplicates (spec says tx IDs are globally unique).
- **HashMap initial capacities**: `transaction_states` and `transaction_amounts` maps could be initialized with configurable capacity hints to reduce reallocations for known workloads.
- **TOML config file**: `TransactionProcessorConfig` currently uses `Default` trait. Could deserialize from a TOML file for deployment flexibility.
- **Metrics**: Transaction count, processing duration, error rate as structured log events.


### AI Disclaimer

The main implementation I wrote entirely myself.
I used Claude Opus 4.6 and ChatGPT 5.4 to bounce ideas off them and myself during planning and design discussions.
I used Claude again for help implementing test code.
I specifically avoided using AI for the main implementation to ensure I have a complete mental model of the project!
I also used Claude for formatting the README!