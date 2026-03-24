use std::collections::BTreeSet;

use crate::convert::rule_engine::{RuleTrace, apply_rules};
use crate::convert::service_refs::rewrite_service_references;
use crate::convert::variables::extract_and_rewrite_variables;
use crate::domain::compose::ComposeProject;
use crate::domain::devfile::{
    Command, Component, ComponentSpec, ContainerComponent, Devfile, Endpoint, EnvVar, Events,
    ExecCommand, Metadata, VolumeComponent, VolumeMount,
};
use crate::domain::rules::RuleSet;

pub struct ConversionResult {
    pub devfile: Devfile,
    pub diagnostics: Vec<String>,
    pub rule_traces: Vec<RuleTrace>,
}

pub fn convert_to_devfile(
    mut project: ComposeProject,
    rules: RuleSet,
    ide_image_override: Option<String>,
) -> ConversionResult {
    let mut components = Vec::new();
    let mut commands = Vec::new();
    let mut diagnostics = Vec::new();
    let mut named_volumes = BTreeSet::<String>::new();
    let mut rule_traces = Vec::new();

    for (service_name, service) in &mut project.services {
        let traces = apply_rules(service_name, service, &rules);
        rule_traces.extend(traces);
    }

    // Extract ${VAR} references and rewrite to {{VAR}} Devfile syntax
    let variables = extract_and_rewrite_variables(&mut project);

    // Replace inter-service hostname references with localhost
    let ref_traces = rewrite_service_references(&mut project);
    rule_traces.extend(ref_traces);

    for (service_name, service) in &project.services {
        let Some(image) = service.image.clone() else {
            if service.build.is_some() {
                diagnostics.push(format!(
                    "Service `{service_name}` only defines `build`; image-less build translation is not implemented yet."
                ));
            }
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

        let endpoints =
            map_ports_to_endpoints(&service.ports, &mut diagnostics, service_name, true);
        let volume_mounts =
            map_volumes_to_mounts(&service.volumes, &mut named_volumes, service_name);

        components.push(Component {
            name: service_name.clone(),
            spec: ComponentSpec::Container(ContainerComponent {
                image,
                env,
                endpoints,
                volume_mounts,
                command,
                args,
                mount_sources: true,
                memory_limit: None,
            }),
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
                        command_line: command_line.clone(),
                        working_dir: service.working_dir.clone(),
                    },
                });

                // Generate a debug variant when ports are exposed
                if !service.ports.is_empty() {
                    commands.push(Command {
                        id: format!("debug-{service_name}"),
                        exec: ExecCommand {
                            component: service_name.clone(),
                            command_line,
                            working_dir: service.working_dir.clone(),
                        },
                    });
                }
            }
        }
    }

    for vol_name in &named_volumes {
        components.push(Component {
            name: vol_name.clone(),
            spec: ComponentSpec::Volume(VolumeComponent { size: None }),
        });
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
            .unwrap_or_else(|| String::from("tool"));
        let ide_name = resolve_component_name(&ide_name, &components);
        let ide_memory_limit = rules
            .base_ide_container
            .as_ref()
            .and_then(|c| c.memory_limit.clone());

        rule_traces.push(RuleTrace {
            service: ide_name.clone(),
            description: format!("Tool container inserted with image {image}"),
        });

        // Insert tool container at the top of the components list
        components.insert(
            0,
            Component {
                name: ide_name,
                spec: ComponentSpec::Container(ContainerComponent {
                    image,
                    env: Vec::new(),
                    endpoints: Vec::new(),
                    volume_mounts: Vec::new(),
                    command: None,
                    args: None,
                    mount_sources: true,
                    memory_limit: ide_memory_limit,
                }),
            },
        );
    } else {
        diagnostics.push(String::from(
            "No tool container configured. Provide one in the input or rules.",
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
            variables,
            components,
            commands,
            events,
        },
        diagnostics,
        rule_traces,
    }
}

fn map_ports_to_endpoints(
    ports: &[crate::domain::compose::ComposePort],
    diagnostics: &mut Vec<String>,
    service_name: &str,
    infer_exposure: bool,
) -> Vec<Endpoint> {
    let mut endpoints = Vec::new();

    for port in ports {
        let port_str = port.container.split('-').next().unwrap_or(&port.container);

        match port_str.parse::<u16>() {
            Ok(target_port) => {
                if port.container.contains('-') {
                    diagnostics.push(format!(
                        "Service `{service_name}` port range `{}` mapped to first port {target_port} only.",
                        port.container
                    ));
                }
                let exposure = if infer_exposure {
                    Some(if port.host.is_some() {
                        String::from("public")
                    } else {
                        String::from("internal")
                    })
                } else {
                    None
                };
                endpoints.push(Endpoint {
                    name: format!("port-{target_port}"),
                    target_port,
                    exposure,
                    protocol: port.protocol.clone(),
                });
            }
            Err(_) => {
                diagnostics.push(format!(
                    "Service `{service_name}` port `{}` could not be parsed as a number.",
                    port.container
                ));
            }
        }
    }

    endpoints
}

fn map_volumes_to_mounts(
    volumes: &[crate::domain::compose::ComposeVolumeMount],
    named_volumes: &mut BTreeSet<String>,
    service_name: &str,
) -> Vec<VolumeMount> {
    let mut mounts = Vec::new();

    for vol in volumes {
        match &vol.source {
            Some(source) if is_bind_mount_source(source) => {
                // Bind mounts are covered by mountSources
            }
            Some(source) => {
                named_volumes.insert(source.clone());
                mounts.push(VolumeMount {
                    name: source.clone(),
                    path: vol.target.clone(),
                });
            }
            None => {
                let auto_name = format!("{service_name}-vol-{}", mounts.len());
                named_volumes.insert(auto_name.clone());
                mounts.push(VolumeMount {
                    name: auto_name,
                    path: vol.target.clone(),
                });
            }
        }
    }

    mounts
}

fn is_bind_mount_source(source: &str) -> bool {
    source.starts_with('.') || source.starts_with('/') || source.starts_with('~')
}

fn resolve_component_name(base_name: &str, components: &[Component]) -> String {
    if !components
        .iter()
        .any(|component| component.name == base_name)
    {
        return base_name.to_string();
    }

    let fallback = format!("{base_name}-base");
    if !components
        .iter()
        .any(|component| component.name == fallback)
    {
        return fallback;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base_name}-base-{index}");
        if !components
            .iter()
            .any(|component| component.name == candidate)
        {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::compose::{ComposePort, ComposeProject, ComposeService, ComposeVolumeMount};
    use crate::domain::devfile::ComponentSpec;
    use crate::domain::rules::{IdeContainerRule, RuleSet};

    use super::convert_to_devfile;

    #[test]
    fn renames_ide_component_when_service_name_conflicts() {
        let project = ComposeProject {
            name: Some(String::from("sample")),
            services: BTreeMap::from([(
                String::from("tool"),
                ComposeService {
                    image: Some(String::from("registry/app:latest")),
                    ..Default::default()
                },
            )]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let result = convert_to_devfile(
            project,
            RuleSet {
                base_ide_container: Some(IdeContainerRule {
                    name: String::from("tool"),
                    image: String::from("quay.io/devfile/udi:latest"),
                    memory_limit: None,
                }),
                ..Default::default()
            },
            None,
        );

        assert!(result.devfile.components.iter().any(|c| c.name == "tool"));
        assert!(
            result
                .devfile
                .components
                .iter()
                .any(|c| c.name == "tool-base")
        );
        // Tool container should be first
        assert_eq!(result.devfile.components[0].name, "tool-base");
    }

    #[test]
    fn maps_ports_to_endpoints_and_volumes_to_components() {
        let project = ComposeProject {
            name: Some(String::from("vol-test")),
            services: BTreeMap::from([(
                String::from("app"),
                ComposeService {
                    image: Some(String::from("myapp:latest")),
                    ports: vec![
                        ComposePort {
                            host: Some(String::from("8080")),
                            container: String::from("80"),
                            protocol: Some(String::from("tcp")),
                        },
                        ComposePort {
                            host: None,
                            container: String::from("443"),
                            protocol: None,
                        },
                    ],
                    volumes: vec![
                        ComposeVolumeMount {
                            source: Some(String::from(".")),
                            target: String::from("/workspace"),
                            read_only: false,
                        },
                        ComposeVolumeMount {
                            source: Some(String::from("data")),
                            target: String::from("/var/lib/data"),
                            read_only: false,
                        },
                        ComposeVolumeMount {
                            source: None,
                            target: String::from("/tmp/scratch"),
                            read_only: false,
                        },
                    ],
                    ..Default::default()
                },
            )]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let result = convert_to_devfile(project, RuleSet::default(), None);

        let app = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "app")
            .unwrap();
        if let ComponentSpec::Container(ref container) = app.spec {
            assert_eq!(container.endpoints.len(), 2);
            assert_eq!(container.endpoints[0].target_port, 80);
            assert_eq!(container.endpoints[0].protocol.as_deref(), Some("tcp"));
            assert_eq!(container.endpoints[0].exposure.as_deref(), Some("public"));
            assert_eq!(container.endpoints[1].target_port, 443);
            assert_eq!(container.endpoints[1].exposure.as_deref(), Some("internal"));

            assert_eq!(container.volume_mounts.len(), 2);
            assert_eq!(container.volume_mounts[0].name, "data");
            assert_eq!(container.volume_mounts[0].path, "/var/lib/data");
            assert_eq!(container.volume_mounts[1].name, "app-vol-1");
            assert_eq!(container.volume_mounts[1].path, "/tmp/scratch");
        } else {
            panic!("expected container component");
        }

        assert!(
            result
                .devfile
                .components
                .iter()
                .any(|c| c.name == "data" && matches!(c.spec, ComponentSpec::Volume(_)))
        );
        assert!(
            result
                .devfile
                .components
                .iter()
                .any(|c| c.name == "app-vol-1" && matches!(c.spec, ComponentSpec::Volume(_)))
        );
    }
}
