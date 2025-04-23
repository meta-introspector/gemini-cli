use colored::*;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
use syntect::easy::HighlightLines;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use std::fmt::Write;

/// Print formatted response from HAPPE daemon to the terminal
pub fn print_happe_response(response: &str) {
    // Check if this is a tool execution response by looking for JSON structure
    if response.trim().starts_with('{') && (response.contains("\"tool\":") || response.contains("\"arguments\":")) {
        println!("\n{}: This query resulted in tool execution:", "Tool Call".blue().bold());
        
        // Try to pretty-print the JSON
        match serde_json::from_str::<serde_json::Value>(response) {
            Ok(json) => {
                if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                    // Split by lines and add prefix to each line
                    for line in pretty.lines() {
                        println!("  {}", line);
                    }
                } else {
                    // Fall back to raw display if pretty printing fails
                    println!("  {}", response);
                }
            },
            Err(_) => {
                // Not valid JSON, just print as-is
                println!("  {}", response);
            }
        }
        println!("\n{}: Tool execution completed.", "System".yellow());
    } else {
        // Regular text response - use markdown rendering
        let rendered_response = render_markdown(response);
        println!("\n{}: {}", "Assistant".cyan().bold(), rendered_response);
    }
}

/// Render markdown in the response
fn render_markdown(text: &str) -> String {
    // This is a simplified implementation
    // For a full implementation, consider using a markdown crate
    
    // Highlight code blocks if possible
    let mut result = String::new();
    let mut in_code_block = false;
    let mut language = String::new();
    let mut code_block = String::new();
    
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];
    
    for line in text.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;
                
                // Highlight the code block if possible
                if let Some(syntax) = ps.find_syntax_by_token(&language) {
                    let mut highlighter = HighlightLines::new(syntax, theme);
                    
                    for line in LinesWithEndings::from(&code_block) {
                        match highlighter.highlight_line(line, &ps) {
                            Ok(highlighted) => {
                                result.push_str(&as_24_bit_terminal_escaped(&highlighted, false));
                            },
                            Err(_) => {
                                // Fallback if highlighting fails
                                result.push_str(line);
                            }
                        }
                    }
                } else {
                    // Just append the code block if we can't highlight it
                    result.push_str(&code_block);
                }
                
                result.push('\n');
                language.clear();
                code_block.clear();
            } else {
                // Start of code block
                in_code_block = true;
                language = line.trim_start_matches("```").to_string();
            }
        } else if in_code_block {
            // Inside code block
            code_block.push_str(line);
            code_block.push('\n');
        } else {
            // Regular text
            // Format bold and italic text
            let formatted = line
                .replace("**", "\x1b[1m")
                .replace("__", "\x1b[1m")
                .replace("*", "\x1b[3m")
                .replace("_", "\x1b[3m")
                .replace("\x1b[1m\x1b[1m", "\x1b[22m")
                .replace("\x1b[3m\x1b[3m", "\x1b[23m");
            
            result.push_str(&formatted);
            result.push('\n');
        }
    }
    
    result
}

/// Show usage instructions when no prompt or action is provided
pub fn print_usage_instructions() {
    println!("{}", "Usage:".yellow().bold());
    println!("  {}", "gemini-cli \"your prompt\"".green().bold());
    println!("    Send a single query to the HAPPE daemon");
    println!();
    println!("  {}", "gemini-cli -i".green().bold());
    println!("    Start an interactive chat session with the HAPPE daemon");
    println!();
    println!("{}", "Options:".cyan());
    println!("  --happe-ipc-path <PATH>  Specify HAPPE daemon socket path");
    println!("  --help                   Show this help message");
    println!();
}
