use colored::*;
use std::env;

// Simple logging functions
// In a more advanced setup, we might use the `log` crate

pub fn log_info(message: &str) {
    if env::var("GEMINI_DEBUG").is_ok() {
        eprintln!("{} {}", "[INFO]".cyan(), message);
    }
}

pub fn log_error(message: &str) {
    eprintln!("{} {}", "[ERROR]".red().bold(), message);
}
