use crate::domain::devfile::{ComponentSpec, Devfile};

pub fn validate_devfile(devfile: &Devfile) -> Vec<String> {
    let mut findings = Vec::new();

    // Schema version check
    if devfile.schema_version != "2.3.0" {
        findings.push(String::from(
            "Output schemaVersion is not 2.3.0, which is outside configured target.",
        ));
    }

    // Must have at least one component
    if devfile.components.is_empty() {
        findings.push(String::from(
            "No container components were generated from the Compose input.",
        ));
    }

    // Metadata name required and non-empty
    if devfile.metadata.name.trim().is_empty() {
        findings.push(String::from(
            "Devfile metadata.name is empty; a valid name is required.",
        ));
    }

    for component in &devfile.components {
        // Component names must be non-empty
        if component.name.trim().is_empty() {
            findings.push(String::from("A component has an empty name field."));
        }

        if let ComponentSpec::Container(ref container) = component.spec {
            // Container image required
            if container.image.trim().is_empty() {
                findings.push(format!(
                    "Component `{}` has an empty image field.",
                    component.name
                ));
            }

            // Endpoint names must be unique within a container
            let mut seen_endpoints = Vec::new();
            for ep in &container.endpoints {
                if seen_endpoints.contains(&ep.name) {
                    findings.push(format!(
                        "Component `{}` has duplicate endpoint name `{}`.",
                        component.name, ep.name
                    ));
                }
                seen_endpoints.push(ep.name.clone());

                // target_port must be in valid range (1-65535)
                if ep.target_port == 0 {
                    findings.push(format!(
                        "Component `{}` endpoint `{}` has target_port 0, which is invalid.",
                        component.name, ep.name
                    ));
                }
            }

            // Volume mount paths must be non-empty
            for vm in &container.volume_mounts {
                if vm.path.trim().is_empty() {
                    findings.push(format!(
                        "Component `{}` has a volume mount with an empty path.",
                        component.name
                    ));
                }
                if vm.name.trim().is_empty() {
                    findings.push(format!(
                        "Component `{}` has a volume mount with an empty name.",
                        component.name
                    ));
                }
            }
        }
    }

    // Duplicate component names
    for (index, component) in devfile.components.iter().enumerate() {
        if devfile
            .components
            .iter()
            .skip(index + 1)
            .any(|other| other.name == component.name)
        {
            findings.push(format!(
                "Component name `{}` is duplicated in the generated Devfile.",
                component.name
            ));
        }
    }

    // Command IDs must be non-empty and unique
    for (index, command) in devfile.commands.iter().enumerate() {
        if command.id.trim().is_empty() {
            findings.push(String::from("A command has an empty id field."));
        }
        if devfile
            .commands
            .iter()
            .skip(index + 1)
            .any(|other| other.id == command.id)
        {
            findings.push(format!(
                "Command id `{}` is duplicated in the generated Devfile.",
                command.id
            ));
        }

        // Command must reference an existing component
        if !devfile
            .components
            .iter()
            .any(|c| c.name == command.exec.component)
        {
            findings.push(format!(
                "Command `{}` references component `{}` which does not exist.",
                command.id, command.exec.component
            ));
        }
    }

    // Events must reference existing commands
    if let Some(events) = &devfile.events {
        for event_cmd in &events.post_start {
            if !devfile.commands.iter().any(|c| c.id == *event_cmd) {
                findings.push(format!(
                    "Event postStart references command `{event_cmd}` which does not exist.",
                ));
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::devfile::*;

    use super::validate_devfile;

    #[test]
    fn valid_devfile_produces_no_findings() {
        let devfile = Devfile {
            schema_version: String::from("2.3.0"),
            metadata: Metadata {
                name: String::from("test"),
            },
            variables: BTreeMap::new(),
            components: vec![Component {
                name: String::from("app"),
                spec: ComponentSpec::Container(ContainerComponent {
                    image: String::from("nginx:latest"),
                    env: Vec::new(),
                    endpoints: vec![Endpoint {
                        name: String::from("http"),
                        target_port: 8080,
                        exposure: None,
                        protocol: None,
                    }],
                    volume_mounts: Vec::new(),
                    command: None,
                    args: None,
                    mount_sources: true,
                    memory_limit: None,
                }),
            }],
            commands: vec![Command {
                id: String::from("run"),
                exec: ExecCommand {
                    component: String::from("app"),
                    command_line: String::from("npm start"),
                    working_dir: None,
                },
            }],
            events: Some(Events {
                post_start: vec![String::from("run")],
            }),
        };
        assert!(validate_devfile(&devfile).is_empty());
    }

    #[test]
    fn catches_empty_image_and_duplicate_names() {
        let devfile = Devfile {
            schema_version: String::from("2.3.0"),
            metadata: Metadata {
                name: String::from("test"),
            },
            variables: BTreeMap::new(),
            components: vec![
                Component {
                    name: String::from("dup"),
                    spec: ComponentSpec::Container(ContainerComponent {
                        image: String::from(""),
                        env: Vec::new(),
                        endpoints: Vec::new(),
                        volume_mounts: Vec::new(),
                        command: None,
                        args: None,
                        mount_sources: true,
                        memory_limit: None,
                    }),
                },
                Component {
                    name: String::from("dup"),
                    spec: ComponentSpec::Container(ContainerComponent {
                        image: String::from("valid:latest"),
                        env: Vec::new(),
                        endpoints: Vec::new(),
                        volume_mounts: Vec::new(),
                        command: None,
                        args: None,
                        mount_sources: true,
                        memory_limit: None,
                    }),
                },
            ],
            commands: Vec::new(),
            events: None,
        };

        let findings = validate_devfile(&devfile);
        assert!(findings.iter().any(|f| f.contains("empty image")));
        assert!(findings.iter().any(|f| f.contains("duplicated")));
    }

    #[test]
    fn catches_orphan_command_reference() {
        let devfile = Devfile {
            schema_version: String::from("2.3.0"),
            metadata: Metadata {
                name: String::from("test"),
            },
            variables: BTreeMap::new(),
            components: vec![Component {
                name: String::from("app"),
                spec: ComponentSpec::Container(ContainerComponent {
                    image: String::from("nginx:latest"),
                    env: Vec::new(),
                    endpoints: Vec::new(),
                    volume_mounts: Vec::new(),
                    command: None,
                    args: None,
                    mount_sources: true,
                    memory_limit: None,
                }),
            }],
            commands: vec![Command {
                id: String::from("run"),
                exec: ExecCommand {
                    component: String::from("nonexistent"),
                    command_line: String::from("npm start"),
                    working_dir: None,
                },
            }],
            events: Some(Events {
                post_start: vec![String::from("run"), String::from("ghost")],
            }),
        };

        let findings = validate_devfile(&devfile);
        assert!(
            findings
                .iter()
                .any(|f| f.contains("references component `nonexistent`"))
        );
        assert!(
            findings
                .iter()
                .any(|f| f.contains("references command `ghost`"))
        );
    }

    #[test]
    fn catches_duplicate_endpoint_names() {
        let devfile = Devfile {
            schema_version: String::from("2.3.0"),
            metadata: Metadata {
                name: String::from("test"),
            },
            variables: BTreeMap::new(),
            components: vec![Component {
                name: String::from("app"),
                spec: ComponentSpec::Container(ContainerComponent {
                    image: String::from("nginx:latest"),
                    env: Vec::new(),
                    endpoints: vec![
                        Endpoint {
                            name: String::from("http"),
                            target_port: 8080,
                            exposure: None,
                            protocol: None,
                        },
                        Endpoint {
                            name: String::from("http"),
                            target_port: 8081,
                            exposure: None,
                            protocol: None,
                        },
                    ],
                    volume_mounts: Vec::new(),
                    command: None,
                    args: None,
                    mount_sources: true,
                    memory_limit: None,
                }),
            }],
            commands: Vec::new(),
            events: None,
        };

        let findings = validate_devfile(&devfile);
        assert!(findings.iter().any(|f| f.contains("duplicate endpoint")));
    }
}
