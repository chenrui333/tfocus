mod cli;
mod display;
mod error;
mod executor;
mod project;
mod selector;
mod types;

use clap::Parser;
use std::path::Path;

use crate::cli::Cli;
use crate::display::Display;
use crate::error::{Result, TfocusError};
use crate::project::TerraformProject;
use crate::selector::{SelectItem, Selector};
use crate::types::{Resource, Target};

#[derive(Debug)]
enum SelectionItem {
    File(usize, std::path::PathBuf),
    Module(usize, String),
    Resource(usize, Resource),
}

fn create_selection_items(selection_items: &[SelectionItem]) -> Vec<SelectItem> {
    selection_items
        .iter()
        .map(|item| {
            let (display, search_text) = match item {
                SelectionItem::File(idx, path) => {
                    let path_str = path.display().to_string();
                    (
                        format!("{:4} {:15} {}", idx, "[File]", path_str),
                        path_str.clone(),
                    )
                }
                SelectionItem::Module(idx, name) => (
                    format!("{:4} {:15} {}", idx, "[Module]", name),
                    name.clone(),
                ),
                SelectionItem::Resource(idx, resource) => {
                    let resource_str = if resource.is_module {
                        format!("module.{}", resource.name)
                    } else {
                        format!("{}.{}", resource.resource_type, resource.name)
                    };
                    (
                        format!(
                            "{:4} {:15} {}",
                            idx,
                            if resource.is_module {
                                "[Module]"
                            } else {
                                "[Resource]"
                            },
                            resource_str
                        ),
                        resource_str,
                    )
                }
            };
            SelectItem {
                display,
                search_text,
                data: match item {
                    SelectionItem::File(_, path) => {
                        format!("f:{}", path.display())
                    }
                    SelectionItem::Module(_, name) => {
                        format!("m:{}", name)
                    }
                    SelectionItem::Resource(_, resource) => {
                        if resource.is_module {
                            format!("m:{}", resource.name)
                        } else {
                            format!("r:{}.{}", resource.resource_type, resource.name)
                        }
                    }
                },
            }
        })
        .collect()
}

fn main() -> Result<()> {
    // setting env
    env_logger::init();
    let cli = Cli::parse();

    if cli.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }

    // Parse the Terraform project
    let project = match TerraformProject::parse_directory(Path::new(&cli.path)) {
        Ok(project) => project,
        Err(TfocusError::NoTerraformFiles) => {
            eprintln!("Error: No Terraform files found in the current directory or its children.");
            eprintln!("Please run this command from a directory containing Terraform files.");
            std::process::exit(1);
        }
        Err(e) => return Err(e),
    };

    // Collect all targets
    let mut selection_items = Vec::new();
    let mut current_index = 1;

    // add files
    for file in project.get_unique_files() {
        selection_items.push(SelectionItem::File(current_index, file));
        current_index += 1;
    }

    // add modules
    for module in project.get_modules() {
        selection_items.push(SelectionItem::Module(current_index, module));
        current_index += 1;
    }

    // add resources
    for resource in project.get_all_resources() {
        selection_items.push(SelectionItem::Resource(current_index, resource));
        current_index += 1;
    }

    // Initialize and run the selector
    let selector_items = create_selection_items(&selection_items);
    let mut selector = Selector::new(selector_items);

    let selected = match selector.run()? {
        Some(data) => data,
        None => {
            println!("\nOperation cancelled");
            std::process::exit(0);
        }
    };

    // Analysis of the selected item
    let target = if let Some(stripped) = selected.strip_prefix("f:") {
        let path = Path::new(stripped).to_path_buf();
        Target::File(path)
    } else if let Some(stripped) = selected.strip_prefix("m:") {
        Target::Module(stripped.to_string())
    } else if let Some(stripped) = selected.strip_prefix("r:") {
        let parts: Vec<&str> = stripped.split('.').collect();
        if parts.len() != 2 {
            return Err(TfocusError::InvalidTargetSelection);
        }
        Target::Resource(parts[0].to_string(), parts[1].to_string())
    } else {
        return Err(TfocusError::InvalidTargetSelection);
    };

    // Get the resources for the selected target
    let resources = match &target {
        Target::File(path) => project.get_resources_by_target(&Target::File(path.clone())),
        Target::Module(name) => project.get_resources_by_target(&Target::Module(name.clone())),
        Target::Resource(resource_type, name) => {
            project.get_resources_by_target(&Target::Resource(resource_type.clone(), name.clone()))
        }
    };

    if resources.is_empty() {
        println!("\nNo resources found for the selected target.");
        return Ok(());
    }

    Display::print_header("\nSelected resources:");
    for resource in &resources {
        Display::print_resource(resource);
    }

    println!();
    // Execute the selected resources
    executor::execute_with_resources(&resources)
}
