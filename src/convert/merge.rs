use std::collections::BTreeMap;

use crate::domain::compose::{ComposeProject, ComposeService};

pub fn merge_projects(projects: Vec<ComposeProject>) -> ComposeProject {
    let mut merged = ComposeProject {
        name: None,
        services: BTreeMap::new(),
        unsupported: Vec::new(),
        includes: Vec::new(),
    };

    for project in projects {
        if project.name.is_some() {
            merged.name = project.name;
        }

        merged.unsupported.extend(project.unsupported);

        for (service_name, incoming) in project.services {
            match merged.services.get_mut(&service_name) {
                Some(existing) => merge_service(existing, incoming),
                None => {
                    merged.services.insert(service_name, incoming);
                }
            }
        }
    }

    merged
}

fn merge_service(existing: &mut ComposeService, incoming: ComposeService) {
    if incoming.image.is_some() {
        existing.image = incoming.image;
    }

    if incoming.build.is_some() {
        existing.build = incoming.build;
    }

    for (k, v) in incoming.environment {
        existing.environment.insert(k, v);
    }

    if !incoming.ports.is_empty() {
        existing.ports = incoming.ports;
    }

    if !incoming.volumes.is_empty() {
        existing.volumes = incoming.volumes;
    }

    if !incoming.command.is_empty() {
        existing.command = incoming.command;
    }

    if !incoming.entrypoint.is_empty() {
        existing.entrypoint = incoming.entrypoint;
    }

    if incoming.working_dir.is_some() {
        existing.working_dir = incoming.working_dir;
    }

    if !incoming.depends_on.is_empty() {
        existing.depends_on = incoming.depends_on;
    }

    if !incoming.post_start.is_empty() {
        existing.post_start = incoming.post_start;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::compose::{ComposeProject, ComposeService};

    use super::merge_projects;

    #[test]
    fn later_documents_override_scalars_and_merge_env() {
        let mut first_service = ComposeService {
            image: Some(String::from("nginx:1.25")),
            ..Default::default()
        };
        first_service
            .environment
            .insert(String::from("A"), String::from("1"));

        let mut second_service = ComposeService {
            image: Some(String::from("nginx:1.26")),
            ..Default::default()
        };
        second_service
            .environment
            .insert(String::from("A"), String::from("2"));
        second_service
            .environment
            .insert(String::from("B"), String::from("3"));

        let first = ComposeProject {
            name: Some(String::from("a")),
            services: BTreeMap::from([(String::from("web"), first_service)]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };
        let second = ComposeProject {
            name: Some(String::from("b")),
            services: BTreeMap::from([(String::from("web"), second_service)]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let merged = merge_projects(vec![first, second]);
        let web = merged.services.get("web").expect("service exists");

        assert_eq!(merged.name.as_deref(), Some("b"));
        assert_eq!(web.image.as_deref(), Some("nginx:1.26"));
        assert_eq!(web.environment.get("A").map(String::as_str), Some("2"));
        assert_eq!(web.environment.get("B").map(String::as_str), Some("3"));
    }
}
