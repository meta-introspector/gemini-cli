use colored::*;
use pulldown_cmark::{Parser as MdParser, Event as MdEvent, Tag, Options, HeadingLevel, CodeBlockKind};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use std::error::Error;
use std::process::Command;
use dialoguer::{Confirm, theme::ColorfulTheme};

/// Print formatted Gemini response to the terminal
pub fn print_gemini_response(response: &str, is_command: bool) {
    if is_command {
        // Just print the raw command suggestion without rendering
        println!("{}: `{}`", "Suggested command".cyan(), response.trim().green());
    } else {
        // Render markdown in the response for normal chat
        let rendered_response = render_markdown(response);
        
        // Print the response with a colored prefix
        println!("{}: {}", "Gemini".blue().bold(), rendered_response);
    }
}

/// Handle command suggestion and potential execution
pub fn handle_command_confirmation(potential_command: &str) -> Result<(), Box<dyn Error>> {
    if potential_command.is_empty() {
        eprintln!("{}", "Gemini did not suggest a command.".yellow());
        return Ok(());
    }
    
    println!("{}: `{}`", "Suggested command".cyan(), potential_command.green());
    
    if Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to run this command?")
        .default(true)
        .interact()? 
    {
        println!("{}", "Executing command...".cyan());
        let output = Command::new("sh")
            .arg("-c")
            .arg(potential_command)
            .output()?; // Use output() to capture stdout/stderr

        if output.status.success() {
            println!("{}", "Command executed successfully:".green());
            if !output.stdout.is_empty() {
                println!("{}", String::from_utf8_lossy(&output.stdout).blue());
            }
        } else {
            eprintln!("{}", "Command failed:".red());
            if !output.stderr.is_empty() {
                 eprintln!("{}", String::from_utf8_lossy(&output.stderr).red());
            }
        }
    } else {
        println!("{}", "Command not executed.".yellow());
    }
    
    Ok(())
}

/// Show usage instructions when no prompt or action is provided
pub fn print_usage_instructions() {
    println!("{}", "No prompt provided. Here are your options:".yellow());
    println!("  {}", "gemini \"your prompt\"".green().bold());
    println!("    Ask Gemini a single question or give it a task");
    println!();
    println!("  {}", "gemini -i".green().bold());
    println!("    Start an interactive chat session with Gemini");
    println!();
    println!("  {}", "gemini -t \"your task description\"".green().bold());
    println!("    Start a task loop where Gemini works on a specific task until completion");
    println!();
    println!("{}", "Configuration options:".cyan());
    println!("  {}", "--set-api-key, --set-system-prompt, --show-config");
    println!();
    println!("{}", "Use --help for more options.".cyan());
}

/// Render markdown in the terminal with syntax highlighting
pub fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    
    let parser = MdParser::new_ext(markdown, options);
    
    // Initialize syntax highlighting
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();
    let theme = &theme_set.themes["base16-ocean.dark"];
    
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_content = String::new();
    let mut output = String::new();
    
    // Table state tracking
    let mut in_table = false;
    let mut in_table_cell = false;
    let mut current_row: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    
    for event in parser {
        match event {
            // Table handling
            MdEvent::Start(Tag::Table(_)) => {
                in_table = true;
                table_rows.clear();
                output.push_str("\n");
            },
            MdEvent::End(Tag::Table(_)) => {
                if !table_rows.is_empty() {
                    // Calculate column widths
                    let col_count = table_rows.iter().map(|row| row.len()).max().unwrap_or(0);
                    let mut col_widths = vec![0; col_count];
                    
                    for row in &table_rows {
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_widths.len() {
                                col_widths[i] = col_widths[i].max(cell.len());
                            }
                        }
                    }
                    
                    // Render table with proper spacing
                    for (i, row) in table_rows.iter().enumerate() {
                        // Print cells with proper padding
                        for (j, cell) in row.iter().enumerate() {
                            if j < col_widths.len() {
                                let padding = col_widths[j].saturating_sub(cell.len());
                                let formatted_cell = if i == 0 {
                                    // Header cell
                                    format!("{}{} ", cell.bold(), " ".repeat(padding))
                                } else {
                                    format!("{}{} ", cell, " ".repeat(padding))
                                };
                                output.push_str(&formatted_cell);
                            }
                        }
                        output.push_str("\n");
                        
                        // Add separator line after header
                        if i == 0 {
                            for (j, width) in col_widths.iter().enumerate() {
                                output.push_str(&"─".repeat(*width).dimmed().to_string());
                                if j < col_widths.len() - 1 {
                                    output.push_str(" ");
                                }
                            }
                            output.push_str("\n");
                        }
                    }
                    output.push_str("\n");
                }
                in_table = false;
            },
            MdEvent::Start(Tag::TableHead) => {
                // Just track that we're in a table header
            },
            MdEvent::End(Tag::TableHead) => {
                // No action needed
            },
            MdEvent::Start(Tag::TableRow) => {
                current_row.clear();
            },
            MdEvent::End(Tag::TableRow) => {
                if !current_row.is_empty() {
                    table_rows.push(current_row.clone());
                }
            },
            MdEvent::Start(Tag::TableCell) => {
                in_table_cell = true;
                current_row.push(String::new());
            },
            MdEvent::End(Tag::TableCell) => {
                in_table_cell = false;
            },
            // Regular markdown elements
            MdEvent::Start(Tag::Heading(level, ..)) => {
                match level {
                    HeadingLevel::H1 => output.push_str(&format!("\n{} ", "##".bright_cyan().bold())),
                    HeadingLevel::H2 => output.push_str(&format!("\n{} ", "#".bright_cyan().bold())),
                    _ => output.push_str("\n"),
                }
            },
            MdEvent::End(Tag::Heading(..)) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Paragraph) => {
                if !in_table && !output.is_empty() && !output.ends_with("\n\n") && !output.ends_with('\n') {
                    output.push_str("\n\n");
                }
            },
            MdEvent::End(Tag::Paragraph) => {
                if !in_table {
                    output.push_str("\n");
                }
            },
            MdEvent::Start(Tag::BlockQuote) => {
                output.push_str("\n");
            },
            MdEvent::End(Tag::BlockQuote) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::CodeBlock(info)) => {
                in_code_block = true;
                // Extract the language from the code block info
                match info {
                    CodeBlockKind::Fenced(lang) => code_block_lang = lang.to_string(),
                    _ => code_block_lang = String::new(),
                }
                code_block_content.clear();
                output.push_str("\n");
            },
            MdEvent::End(Tag::CodeBlock(_)) => {
                // Apply syntax highlighting
                let syntax = if code_block_lang.is_empty() {
                    syntax_set.find_syntax_plain_text()
                } else {
                    syntax_set.find_syntax_by_token(&code_block_lang)
                        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
                };
                
                let mut highlighter = HighlightLines::new(syntax, theme);
                
                // Add a separator line
                output.push_str(&format!("{}:\n", code_block_lang.cyan()));
                output.push_str(&"─".repeat(40).dimmed().to_string());
                output.push_str("\n");
                
                for line in LinesWithEndings::from(&code_block_content) {
                    let highlighted = highlighter.highlight_line(line, &syntax_set).unwrap_or_default();
                    let escaped = as_24_bit_terminal_escaped(&highlighted, false);
                    output.push_str(&escaped);
                }
                
                // Add a separator line
                output.push_str(&"─".repeat(40).dimmed().to_string());
                output.push_str("\n\n");
                
                in_code_block = false;
            },
            MdEvent::Start(Tag::List(_)) => {
                output.push_str("\n");
            },
            MdEvent::End(Tag::List(_)) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Item) => {
                output.push_str(&format!("{}  ", "•".yellow()));
            },
            MdEvent::End(Tag::Item) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Emphasis) => {
                if !in_table_cell {
                    // No special handling needed for table cells
                }
            },
            MdEvent::End(Tag::Emphasis) => {
                // No special handling needed
            },
            MdEvent::Start(Tag::Strong) => {
                if !in_table_cell {
                    // No special handling needed
                }
            },
            MdEvent::End(Tag::Strong) => {
                // No special handling needed
            },
            MdEvent::Code(ref code) => {
                if in_table_cell && !current_row.is_empty() {
                    let idx = current_row.len() - 1;
                    current_row[idx].push_str(&format!("`{}`", code));
                } else {
                    output.push_str(&format!("`{}`", code.on_bright_black().white()));
                }
            },
            MdEvent::Text(ref text) => {
                if in_code_block {
                    code_block_content.push_str(text);
                } else if in_table_cell && !current_row.is_empty() {
                    let idx = current_row.len() - 1;
                    current_row[idx].push_str(text);
                } else {
                    output.push_str(text);
                }
            },
            MdEvent::Html(ref html) => {
                // Just pass through HTML
                if !in_table_cell {
                    output.push_str(html);
                }
            },
            MdEvent::SoftBreak => {
                if !in_table_cell {
                    output.push(' ');
                }
            },
            MdEvent::HardBreak => {
                if !in_table_cell {
                    output.push('\n');
                }
            },
            _ => {
                // Handle other cases as needed
            }
        }
    }
    
    output
} 