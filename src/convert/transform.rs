use crate::convert::rule_engine::apply_rules;
use crate::domain::compose::ComposeProject;
use crate::domain::devfile::{
    Command, Component, ContainerComponent, Devfile, EnvVar, Events, ExecCommand, Metadata,
};
use crate::domain::rules::RuleSet;

pub struct ConversionResult {
    pub devfile: Devfile,
    pub diagnostics: Vec<String>,
}

pub fn convert_to_devfile(
    mut project: ComposeProject,
    rules: RuleSet,
    ide_image_override: Option<String>,
) -> ConversionResult {
    let mut components = Vec::new();
    let mut commands = Vec::new();
    let mut diagnostics = Vec::new();

    for (service_name, service) in &mut project.services {
        apply_rules(service_name, service, &rules);

        let Some(image) = service.image.clone() else {
            diagnostics.push(format!(
                "Service `{service_name}` has no image and was skipped."
            ));
            continue;
        };

        let env = service
            .environment
            .iter()
            .map(|(name, value)| EnvVar {
                name: name.clone(),
                value: value.clone(),
            })
            .collect::<Vec<_>>();

        let command = if service.entrypoint.is_empty() {
            None
        } else {
            Some(service.entrypoint.clone())
        };

        let args = if service.command.is_empty() {
            None
        } else {
            Some(service.command.clone())
        };

        components.push(Component {
            name: service_name.clone(),
            container: ContainerComponent {
                image,
                env,
                command,
                args,
                mount_sources: true,
                memory_limit: None,
            },
        });

        if !service.command.is_empty() || !service.entrypoint.is_empty() {
            let command_line = service
                .entrypoint
                .iter()
                .chain(service.command.iter())
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ");

            if !command_line.trim().is_empty() {
                commands.push(Command {
                    id: format!("run-{service_name}"),
                    exec: ExecCommand {
                        component: service_name.clone(),
                        command_line,
                        working_dir: service.working_dir.clone(),
                    },
                });
            }
        }
    }

    let ide_image = match ide_image_override {
        Some(raw) if !raw.trim().is_empty() => Some(raw),
        _ => rules.base_ide_container.as_ref().map(|c| c.image.clone()),
    };

    if let Some(image) = ide_image {
        let ide_name = rules
            .base_ide_container
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| String::from("ide"));
        let ide_memory_limit = rules
            .base_ide_container
            .as_ref()
            .and_then(|c| c.memory_limit.clone());

        components.push(Component {
            name: ide_name,
            container: ContainerComponent {
                image,
                env: Vec::new(),
                command: None,
                args: None,
                mount_sources: true,
                memory_limit: ide_memory_limit,
            },
        });
    } else {
        diagnostics.push(String::from(
            "No IDE base container configured. Provide one in the input or rules.",
        ));
    }

    diagnostics.extend(project.unsupported);

    let metadata_name = project
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| String::from("compose-conversion"));

    let events = if commands.is_empty() {
        None
    } else {
        Some(Events {
            post_start: commands.iter().map(|c| c.id.clone()).collect(),
        })
    };

    ConversionResult {
        devfile: Devfile {
            schema_version: String::from("2.3.0"),
            metadata: Metadata {
                name: metadata_name,
            },
            components,
            commands,
            events,
        },
        diagnostics,
    }
}