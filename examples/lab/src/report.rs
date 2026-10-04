//! Output helpers shared by the examples.

/// The result type every example's `main` and sections return.
pub type Res = Result<(), Box<dyn std::error::Error>>;

/// Print a section heading.
pub fn section(title: &str) {
    println!("\n== {title} ==");
}
