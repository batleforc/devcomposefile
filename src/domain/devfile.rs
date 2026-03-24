use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devfile {
    pub schema_version: String,
    pub metadata: Metadata,
    pub components: Vec<Component>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Events>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Metadata {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub name: String,
    pub container: ContainerComponent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerComponent {
    pub image: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    pub mount_sources: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub id: String,
    pub exec: ExecCommand,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCommand {
    pub component: String,
    pub command_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Events {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub post_start: Vec<String>,
}
