// Temporary draft-branch allowance for two duplicated #[expect] attributes in
// parity.rs. Remove the attributes and this allowance before merging PR #18.
#[allow(unfulfilled_lint_expectations)]
mod app;
mod linux;
mod process_table;
mod settings;

fn main() {
    app::run();
}
