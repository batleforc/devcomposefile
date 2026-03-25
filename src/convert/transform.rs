use std::collections::{BTreeMap as StdBTreeMap, BTreeSet};

use crate::convert::rule_engine::{RuleTrace, apply_rules};
use crate::convert::service_refs::rewrite_service_references;
use crate::convert::variables::extract_and_rewrite_variables;
use crate::domain::compose::ComposeProject;
use crate::domain::devfile::{
    Command, Component, ComponentSpec, ContainerComponent, Devfile, Endpoint, EnvVar, Events,
    ExecCommand, Metadata, Parent, VolumeComponent, VolumeMount,
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
    let mut commands: Vec<Command> = Vec::new();
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

    // Pre-scan for container ports used by multiple services
    let duplicate_ports = collect_duplicate_ports(&project);

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

        let has_container_cmd = !service.entrypoint.is_empty() || !service.command.is_empty();

        // When a service has a command or entrypoint, keep the container alive
        // with `tail -f /dev/null` and move the original command into a Devfile
        // postStart Command so it runs after all containers are ready.
        let (command, args) = if has_container_cmd {
            // Build Command from original entrypoint + command
            let mut cmd_parts = service.entrypoint.clone();
            cmd_parts.extend(service.command.clone());
            let command_line = cmd_parts
                .iter()
                .map(|p| {
                    if p.contains(' ') {
                        format!("\"{}\"", p.replace('"', "\\\""))
                    } else {
                        p.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            commands.push(Command {
                id: format!("run-{service_name}"),
                exec: ExecCommand {
                    component: service_name.clone(),
                    command_line,
                    working_dir: service.working_dir.clone(),
                },
            });

            rule_traces.push(RuleTrace {
                service: service_name.clone(),
                description: format!(
                    "Container set to idle, original command moved to postStart run-{service_name}"
                ),
            });

            // Container idles with tail -f /dev/null
            (
                Some(vec![String::from("tail")]),
                Some(vec![String::from("-f"), String::from("/dev/null")]),
            )
        } else {
            (None, None)
        };

        let endpoints = map_ports_to_endpoints(
            &service.ports,
            &mut diagnostics,
            service_name,
            true,
            &duplicate_ports,
        );
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
    }

    for vol_name in &named_volumes {
        components.push(Component {
            name: vol_name.clone(),
            spec: ComponentSpec::Volume(VolumeComponent { size: None }),
        });
    }

    let ide_image = match ide_image_override {
        Some(raw) if !raw.trim().is_empty() => Some(raw),
        _ => None,
    };

    // Decide between: (1) IDE image override, (2) parent devfile, (3) inline baseIdeContainer
    let parent = if ide_image.is_some() {
        // Explicit IDE image override always wins → inline container, no parent
        None
    } else if let Some(ref parent_rule) = rules.parent_devfile {
        let has_content = parent_rule.id.is_some() || parent_rule.uri.is_some();
        if has_content {
            rule_traces.push(RuleTrace {
                service: String::from("parent"),
                description: format!(
                    "Parent devfile reference: {}",
                    parent_rule
                        .id
                        .as_deref()
                        .or(parent_rule.uri.as_deref())
                        .unwrap_or("(empty)")
                ),
            });
            Some(Parent {
                id: parent_rule.id.clone(),
                registry_url: parent_rule.registry_url.clone(),
                uri: parent_rule.uri.clone(),
                version: parent_rule.version.clone(),
            })
        } else {
            None
        }
    } else {
        None
    };

    // Only insert inline IDE container when NOT using a parent devfile
    if parent.is_none() {
        let ide_image_resolved =
            ide_image.or_else(|| rules.base_ide_container.as_ref().map(|c| c.image.clone()));

        if let Some(image) = ide_image_resolved {
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
                "No tool container or parent devfile configured. Provide one in the input or rules.",
            ));
        }
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
            parent,
            variables,
            components,
            commands,
            events,
        },
        diagnostics,
        rule_traces,
    }
}

/// Collect container ports that appear in more than one service.
fn collect_duplicate_ports(project: &ComposeProject) -> BTreeSet<u16> {
    let mut port_owners: StdBTreeMap<u16, usize> = StdBTreeMap::new();
    for service in project.services.values() {
        // Use a set so a service exposing the same port twice only counts once.
        let unique: BTreeSet<u16> = service
            .ports
            .iter()
            .filter_map(|p| p.container.split('-').next().and_then(|s| s.parse().ok()))
            .collect();
        for port in unique {
            *port_owners.entry(port).or_insert(0) += 1;
        }
    }
    port_owners
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(port, _)| port)
        .collect()
}

fn map_ports_to_endpoints(
    ports: &[crate::domain::compose::ComposePort],
    diagnostics: &mut Vec<String>,
    service_name: &str,
    infer_exposure: bool,
    duplicate_ports: &BTreeSet<u16>,
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

                let name = if duplicate_ports.contains(&target_port) {
                    if let Some(host) = &port.host {
                        format!("port-{host}-{target_port}")
                    } else {
                        format!("{service_name}-port-{target_port}")
                    }
                } else {
                    format!("port-{target_port}")
                };

                endpoints.push(Endpoint {
                    name,
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

    #[test]
    fn duplicate_ports_across_services_get_prefixed() {
        let project = ComposeProject {
            name: Some(String::from("dup-ports")),
            services: BTreeMap::from([
                (
                    String::from("frontend"),
                    ComposeService {
                        image: Some(String::from("nginx:latest")),
                        ports: vec![ComposePort {
                            host: Some(String::from("8080")),
                            container: String::from("3000"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
                (
                    String::from("backend"),
                    ComposeService {
                        image: Some(String::from("node:20")),
                        ports: vec![ComposePort {
                            host: Some(String::from("9090")),
                            container: String::from("3000"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
            ]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let result = convert_to_devfile(project, RuleSet::default(), None);

        let backend = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "backend")
            .unwrap();
        let frontend = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "frontend")
            .unwrap();

        if let ComponentSpec::Container(ref ctr) = frontend.spec {
            assert_eq!(ctr.endpoints[0].name, "port-8080-3000");
            assert_eq!(ctr.endpoints[0].target_port, 3000);
        } else {
            panic!("expected container");
        }

        if let ComponentSpec::Container(ref ctr) = backend.spec {
            assert_eq!(ctr.endpoints[0].name, "port-9090-3000");
            assert_eq!(ctr.endpoints[0].target_port, 3000);
        } else {
            panic!("expected container");
        }
    }

    #[test]
    fn duplicate_port_without_host_uses_service_name_prefix() {
        let project = ComposeProject {
            name: Some(String::from("dup-no-host")),
            services: BTreeMap::from([
                (
                    String::from("api"),
                    ComposeService {
                        image: Some(String::from("api:latest")),
                        ports: vec![ComposePort {
                            host: None,
                            container: String::from("8080"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
                (
                    String::from("worker"),
                    ComposeService {
                        image: Some(String::from("worker:latest")),
                        ports: vec![ComposePort {
                            host: None,
                            container: String::from("8080"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
            ]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let result = convert_to_devfile(project, RuleSet::default(), None);

        let api = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "api")
            .unwrap();
        let worker = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "worker")
            .unwrap();

        if let ComponentSpec::Container(ref ctr) = api.spec {
            assert_eq!(ctr.endpoints[0].name, "api-port-8080");
        } else {
            panic!("expected container");
        }

        if let ComponentSpec::Container(ref ctr) = worker.spec {
            assert_eq!(ctr.endpoints[0].name, "worker-port-8080");
        } else {
            panic!("expected container");
        }
    }

    #[test]
    fn unique_ports_not_prefixed() {
        let project = ComposeProject {
            name: Some(String::from("unique-ports")),
            services: BTreeMap::from([
                (
                    String::from("web"),
                    ComposeService {
                        image: Some(String::from("nginx:latest")),
                        ports: vec![ComposePort {
                            host: Some(String::from("8080")),
                            container: String::from("80"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
                (
                    String::from("db"),
                    ComposeService {
                        image: Some(String::from("postgres:16")),
                        ports: vec![ComposePort {
                            host: Some(String::from("5432")),
                            container: String::from("5432"),
                            protocol: None,
                        }],
                        ..Default::default()
                    },
                ),
            ]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let result = convert_to_devfile(project, RuleSet::default(), None);

        let web = result
            .devfile
            .components
            .iter()
            .find(|c| c.name == "web")
            .unwrap();
        if let ComponentSpec::Container(ref ctr) = web.spec {
            assert_eq!(ctr.endpoints[0].name, "port-80");
        } else {
            panic!("expected container");
        }
    }
}
