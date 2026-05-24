#[cfg(feature = "workspaces")]
use super::WorkspaceClient;
#[cfg(feature = "keyboard")]
use super::{KeyboardLayoutClient, KeyboardLayoutUpdate};
use super::{Visibility, Workspace};
use crate::channels::SyncSenderExt;
use crate::spawn;
#[cfg(any(feature = "keyboard", feature = "workspaces"))]
use crate::{read_lock, write_lock};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
#[cfg(any(feature = "keyboard", feature = "workspaces"))]
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{debug, error, warn};

#[cfg(feature = "workspaces")]
use super::WorkspaceUpdate;

#[derive(Debug)]
pub struct Client {
    tx: Sender<ClientUpdate>,
    _rx: Receiver<ClientUpdate>,
    #[cfg(feature = "workspaces")]
    workspaces: Arc<RwLock<Vec<TriadWorkspace>>>,
    #[cfg(feature = "keyboard")]
    keyboard_layout: Arc<RwLock<Option<KeyboardLayoutUpdate>>>,
}

#[derive(Debug, Clone)]
enum ClientUpdate {
    #[cfg(feature = "workspaces")]
    Workspace(WorkspaceUpdate),
    #[cfg(feature = "keyboard")]
    KeyboardLayout(KeyboardLayoutUpdate),
}

impl Client {
    pub(crate) fn new() -> Self {
        let (tx, rx) = broadcast::channel(32);
        let client = Self {
            tx,
            _rx: rx,
            #[cfg(feature = "workspaces")]
            workspaces: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "keyboard")]
            keyboard_layout: Arc::new(RwLock::new(None)),
        };

        client.start();
        client
    }

    fn start(&self) {
        let tx = self.tx.clone();
        #[cfg(feature = "workspaces")]
        let workspaces = self.workspaces.clone();
        #[cfg(feature = "keyboard")]
        let keyboard_layout = self.keyboard_layout.clone();

        spawn(async move {
            #[cfg(feature = "workspaces")]
            let mut old_workspaces = Vec::new();

            Self::load_initial_state(
                &tx,
                #[cfg(feature = "workspaces")]
                &workspaces,
                #[cfg(feature = "keyboard")]
                &keyboard_layout,
                #[cfg(feature = "workspaces")]
                &mut old_workspaces,
            )
            .await;

            loop {
                match EventStream::connect().await {
                    Ok(mut stream) => {
                        while let Some(message) = stream.next().await {
                            match message {
                                Ok(message) => {
                                    Self::apply_event(
                                        &tx,
                                        #[cfg(feature = "workspaces")]
                                        &workspaces,
                                        #[cfg(feature = "keyboard")]
                                        &keyboard_layout,
                                        #[cfg(feature = "workspaces")]
                                        &mut old_workspaces,
                                        message,
                                    );
                                }
                                Err(err) => {
                                    warn!("Triad event stream failed: {err:#}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => warn!("Failed to connect to Triad event stream: {err:#}"),
                }

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    async fn load_initial_state(
        tx: &Sender<ClientUpdate>,
        #[cfg(feature = "workspaces")] workspaces: &Arc<RwLock<Vec<TriadWorkspace>>>,
        #[cfg(feature = "keyboard")] keyboard_layout: &Arc<RwLock<Option<KeyboardLayoutUpdate>>>,
        #[cfg(feature = "workspaces")] old_workspaces: &mut Vec<TriadWorkspace>,
    ) {
        loop {
            match command(StateRequest::State).await {
                Ok(reply) => {
                    Self::apply_state(
                        tx,
                        #[cfg(feature = "workspaces")]
                        workspaces,
                        #[cfg(feature = "keyboard")]
                        keyboard_layout,
                        #[cfg(feature = "workspaces")]
                        old_workspaces,
                        reply.state,
                    );
                    return;
                }
                Err(err) => {
                    warn!("Failed to get Triad state: {err:#}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    fn apply_event(
        tx: &Sender<ClientUpdate>,
        #[cfg(feature = "workspaces")] workspaces: &Arc<RwLock<Vec<TriadWorkspace>>>,
        #[cfg(feature = "keyboard")] keyboard_layout: &Arc<RwLock<Option<KeyboardLayoutUpdate>>>,
        #[cfg(feature = "workspaces")] old_workspaces: &mut Vec<TriadWorkspace>,
        message: EventMessage,
    ) {
        match message {
            EventMessage::State(state) => {
                Self::apply_state(
                    tx,
                    #[cfg(feature = "workspaces")]
                    workspaces,
                    #[cfg(feature = "keyboard")]
                    keyboard_layout,
                    #[cfg(feature = "workspaces")]
                    old_workspaces,
                    state,
                );
            }
            #[cfg(feature = "workspaces")]
            EventMessage::Layout(layout) => {
                Self::apply_workspaces(tx, workspaces, old_workspaces, layout.workspaces);
            }
            #[cfg(not(feature = "workspaces"))]
            EventMessage::Layout(_) => {}
            EventMessage::Unknown(event) => debug!(
                "Ignoring unsupported Triad event: {}",
                event.unwrap_or_else(|| "<unknown>".to_string())
            ),
        }
    }

    fn apply_state(
        tx: &Sender<ClientUpdate>,
        #[cfg(feature = "workspaces")] workspaces: &Arc<RwLock<Vec<TriadWorkspace>>>,
        #[cfg(feature = "keyboard")] keyboard_layout: &Arc<RwLock<Option<KeyboardLayoutUpdate>>>,
        #[cfg(feature = "workspaces")] old_workspaces: &mut Vec<TriadWorkspace>,
        state: RawState,
    ) {
        #[cfg(feature = "keyboard")]
        Self::apply_keyboard_layout(tx, keyboard_layout, state.keyboard_layout());
        #[cfg(feature = "workspaces")]
        Self::apply_workspaces(tx, workspaces, old_workspaces, state.layout.workspaces);
    }

    #[cfg(feature = "workspaces")]
    fn apply_workspaces(
        tx: &Sender<ClientUpdate>,
        workspaces: &Arc<RwLock<Vec<TriadWorkspace>>>,
        old_workspaces: &mut Vec<TriadWorkspace>,
        raw_workspaces: Vec<RawWorkspace>,
    ) {
        let next = raw_workspaces
            .into_iter()
            .map(TriadWorkspace::from)
            .collect::<Vec<_>>();

        let updates = workspace_updates(old_workspaces, &next);
        *old_workspaces = next.clone();
        *write_lock!(workspaces) = next;

        for update in updates {
            tx.send_expect(ClientUpdate::Workspace(update));
        }
    }

    #[cfg(feature = "keyboard")]
    fn apply_keyboard_layout(
        tx: &Sender<ClientUpdate>,
        current: &Arc<RwLock<Option<KeyboardLayoutUpdate>>>,
        layout: Option<KeyboardLayoutUpdate>,
    ) {
        if layout.as_ref().map(|layout| &layout.0)
            == read_lock!(current).as_ref().map(|layout| &layout.0)
        {
            return;
        }

        *write_lock!(current) = layout.clone();

        if let Some(layout) = layout {
            tx.send_expect(ClientUpdate::KeyboardLayout(layout));
        }
    }
}

#[cfg(feature = "workspaces+triad")]
impl WorkspaceClient for Client {
    fn focus(&self, id: i64) {
        spawn(async move {
            let request = StateRequest::Action {
                action: "focus-tag",
                extra: Map::from_iter([("tag".into(), json!(id))]),
            };

            if let Err(err) = command(request).await {
                error!("Failed to focus Triad workspace {id}: {err:#}");
            }
        });
    }

    fn subscribe(&self) -> Receiver<WorkspaceUpdate> {
        let (tx, rx) = broadcast::channel(16);

        {
            let workspaces = read_lock!(self.workspaces);
            if !workspaces.is_empty() {
                tx.send_expect(WorkspaceUpdate::Init(
                    workspaces
                        .iter()
                        .map(|workspace| workspace.workspace.clone())
                        .collect(),
                ));
            }
        }

        let mut source = self.tx.subscribe();
        spawn(async move {
            while let Ok(update) = source.recv().await {
                match update {
                    ClientUpdate::Workspace(update) => tx.send_expect(update),
                    #[cfg(feature = "keyboard")]
                    ClientUpdate::KeyboardLayout(_) => {}
                }
            }

            Ok::<(), std::io::Error>(())
        });

        rx
    }
}

#[cfg(feature = "keyboard+triad")]
impl KeyboardLayoutClient for Client {
    fn set_next_active(&self) {
        spawn(async move {
            let request = StateRequest::Action {
                action: "switch-keyboard-layout",
                extra: Map::from_iter([("layout".into(), json!("next"))]),
            };

            if let Err(err) = command(request).await {
                error!("Failed to switch Triad keyboard layout: {err:#}");
            }
        });
    }

    fn subscribe(&self) -> Receiver<KeyboardLayoutUpdate> {
        let (tx, rx) = broadcast::channel(16);

        if let Some(layout) = read_lock!(self.keyboard_layout).clone() {
            tx.send_expect(layout);
        }

        let mut source = self.tx.subscribe();
        spawn(async move {
            while let Ok(update) = source.recv().await {
                match update {
                    ClientUpdate::KeyboardLayout(update) => tx.send_expect(update),
                    #[cfg(feature = "workspaces")]
                    ClientUpdate::Workspace(_) => {}
                }
            }

            Ok::<(), std::io::Error>(())
        });

        rx
    }
}

#[cfg(feature = "workspaces")]
fn workspace_updates(
    old_workspaces: &[TriadWorkspace],
    new_workspaces: &[TriadWorkspace],
) -> Vec<WorkspaceUpdate> {
    let mut updates = Vec::new();

    if old_workspaces.is_empty() {
        updates.push(WorkspaceUpdate::Init(
            new_workspaces
                .iter()
                .map(|workspace| workspace.workspace.clone())
                .collect(),
        ));
        return updates;
    }

    for triad_workspace in new_workspaces {
        let workspace = &triad_workspace.workspace;
        match old_workspaces
            .iter()
            .find(|old| old.workspace.id == workspace.id)
        {
            None => updates.push(WorkspaceUpdate::Add(workspace.clone())),
            Some(old_workspace) => {
                let old = &old_workspace.workspace;
                if workspace.name != old.name {
                    updates.push(WorkspaceUpdate::Rename {
                        id: workspace.id,
                        name: workspace.name.clone(),
                    });
                }

                if workspace.monitor != old.monitor || workspace.index != old.index {
                    updates.push(WorkspaceUpdate::Move(workspace.clone()));
                }

                if workspace.visibility.is_focused() && !old.visibility.is_focused() {
                    updates.push(WorkspaceUpdate::Focus {
                        old: old_workspaces
                            .iter()
                            .find(|workspace| workspace.workspace.visibility.is_focused())
                            .map(|workspace| workspace.workspace.clone()),
                        new: workspace.clone(),
                    });
                }

                if triad_workspace.urgent != old_workspace.urgent {
                    updates.push(WorkspaceUpdate::Urgent {
                        id: workspace.id,
                        urgent: triad_workspace.urgent,
                    });
                }
            }
        }
    }

    for workspace in old_workspaces {
        if !new_workspaces
            .iter()
            .any(|new| new.workspace.id == workspace.workspace.id)
        {
            updates.push(WorkspaceUpdate::Remove(workspace.workspace.id));
        }
    }

    updates
}

#[derive(Debug)]
enum StateRequest {
    State,
    Action {
        action: &'static str,
        extra: Map<String, Value>,
    },
}

async fn command(request: StateRequest) -> color_eyre::Result<NativeReply> {
    let mut stream = BufReader::new(UnixStream::connect(socket_path()?).await?);
    let request = match request {
        StateRequest::State => json!({"triad":{"version":1,"request":"state"}}),
        StateRequest::Action { action, mut extra } => {
            let mut triad = Map::new();
            triad.insert("version".into(), json!(1));
            triad.insert("request".into(), json!("action"));
            triad.insert("action".into(), json!(action));
            triad.append(&mut extra);
            Value::Object(Map::from_iter([("triad".into(), Value::Object(triad))]))
        }
    };

    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    stream.get_mut().write_all(line.as_bytes()).await?;

    line.clear();
    stream.read_line(&mut line).await?;
    decode_reply(&line)
}

#[derive(Debug)]
struct EventStream {
    reader: BufReader<UnixStream>,
}

impl EventStream {
    async fn connect() -> color_eyre::Result<Self> {
        let mut reader = BufReader::new(UnixStream::connect(socket_path()?).await?);
        let request = json!({
            "triad": {
                "version": 1,
                "request": "event-stream",
                "events": ["state", "layout"]
            }
        });
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        reader.get_mut().write_all(line.as_bytes()).await?;

        line.clear();
        reader.read_line(&mut line).await?;
        decode_reply(&line)?;

        Ok(Self { reader })
    }

    async fn next(&mut self) -> Option<color_eyre::Result<EventMessage>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None,
            Ok(_) => Some(decode_event(&line)),
            Err(err) => Some(Err(err.into())),
        }
    }
}

fn decode_reply(line: &str) -> color_eyre::Result<NativeReply> {
    let reply = serde_json::from_str::<NativeEnvelope>(line)?;
    if !reply.ok {
        color_eyre::eyre::bail!(
            "{}",
            reply
                .error
                .unwrap_or_else(|| "Triad request failed".to_string())
        );
    }

    reply
        .triad
        .ok_or_else(|| color_eyre::eyre::eyre!("missing Triad response"))
}

fn decode_event(line: &str) -> color_eyre::Result<EventMessage> {
    let event = serde_json::from_str::<EventEnvelope>(line)?;
    match event.triad.event.as_deref() {
        Some("state-changed") => {
            let state = event
                .triad
                .state
                .ok_or_else(|| color_eyre::eyre::eyre!("missing Triad state event payload"))?;
            serde_json::from_value(state)
                .map(EventMessage::State)
                .map_err(Into::into)
        }
        Some("layout-state-changed") => {
            let state = event
                .triad
                .state
                .ok_or_else(|| color_eyre::eyre::eyre!("missing Triad layout event payload"))?;
            serde_json::from_value(state)
                .map(EventMessage::Layout)
                .map_err(Into::into)
        }
        event => Ok(EventMessage::Unknown(event.map(ToString::to_string))),
    }
}

fn socket_path() -> color_eyre::Result<PathBuf> {
    if let Ok(path) = std::env::var("TRIAD_SOCKET")
        && !path.is_empty()
    {
        return Ok(path.into());
    }

    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|dir| dir.join("triad.sock"))
        .or_else(|| Some(PathBuf::from("/tmp/triad.sock")))
        .ok_or_else(|| color_eyre::eyre::eyre!("Triad socket path not found"))
}

#[derive(Debug, Deserialize)]
struct NativeEnvelope {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    triad: Option<NativeReply>,
}

#[derive(Debug, Deserialize)]
struct NativeReply {
    #[serde(default)]
    state: RawState,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    triad: EventPayload,
}

#[derive(Debug, Deserialize)]
struct EventPayload {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    state: Option<Value>,
}

#[derive(Debug, Clone)]
enum EventMessage {
    State(RawState),
    Layout(RawLayout),
    Unknown(Option<String>),
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawState {
    #[cfg(feature = "workspaces")]
    #[serde(default)]
    layout: RawLayout,
    #[cfg(feature = "keyboard")]
    #[serde(default, deserialize_with = "deserialize_keyboard_layouts")]
    keyboard_layouts: KeyboardLayouts,
    #[cfg(feature = "keyboard")]
    #[serde(default)]
    current_keyboard_layout_idx: Option<u32>,
}

impl RawState {
    #[cfg(feature = "keyboard")]
    fn keyboard_layout(&self) -> Option<KeyboardLayoutUpdate> {
        let index = self
            .current_keyboard_layout_idx
            .unwrap_or(self.keyboard_layouts.current_idx) as usize;

        self.keyboard_layouts
            .names
            .get(index)
            .cloned()
            .map(KeyboardLayoutUpdate)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawLayout {
    #[serde(default)]
    workspaces: Vec<RawWorkspace>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    tag_id: u64,
    #[serde(default)]
    workspace_idx: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_output_visible: bool,
    #[serde(default)]
    is_urgent: bool,
}

impl From<RawWorkspace> for Workspace {
    fn from(workspace: RawWorkspace) -> Self {
        Self {
            id: workspace.tag_id as i64,
            index: workspace.workspace_idx as i64,
            name: workspace
                .name
                .unwrap_or_else(|| workspace.workspace_idx.to_string()),
            monitor: workspace.output.unwrap_or_default(),
            visibility: if workspace.is_active {
                Visibility::focused()
            } else if workspace.is_output_visible {
                Visibility::visible()
            } else {
                Visibility::Hidden
            },
        }
    }
}

#[derive(Debug, Clone)]
struct TriadWorkspace {
    workspace: Workspace,
    urgent: bool,
}

impl From<RawWorkspace> for TriadWorkspace {
    fn from(workspace: RawWorkspace) -> Self {
        let urgent = workspace.is_urgent;
        Self {
            workspace: Workspace::from(workspace),
            urgent,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct KeyboardLayouts {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    current_idx: u32,
}

fn deserialize_keyboard_layouts<'de, D>(deserializer: D) -> Result<KeyboardLayouts, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Names(Vec<String>),
        Object(KeyboardLayouts),
    }

    match Wire::deserialize(deserializer)? {
        Wire::Names(names) => Ok(KeyboardLayouts {
            names,
            current_idx: 0,
        }),
        Wire::Object(layouts) => Ok(layouts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "keyboard")]
    #[test]
    fn parses_keyboard_layouts_from_vector_form() {
        let state = serde_json::from_str::<RawState>(
            r#"{"keyboard_layouts":["us","de"],"current_keyboard_layout_idx":1}"#,
        )
        .expect("state to parse");

        assert_eq!(
            state.keyboard_layout().map(|layout| layout.0),
            Some("de".into())
        );
    }

    #[cfg(feature = "keyboard")]
    #[test]
    fn parses_keyboard_layouts_from_object_form() {
        let state = serde_json::from_str::<RawState>(
            r#"{"keyboard_layouts":{"names":["us","de"],"current_idx":1}}"#,
        )
        .expect("state to parse");

        assert_eq!(
            state.keyboard_layout().map(|layout| layout.0),
            Some("de".into())
        );
    }

    #[cfg(feature = "workspaces")]
    #[test]
    fn maps_workspace_visibility() {
        let focused = Workspace::from(RawWorkspace {
            tag_id: 1,
            workspace_idx: 1,
            is_active: true,
            ..RawWorkspace::default()
        });
        let visible = Workspace::from(RawWorkspace {
            tag_id: 2,
            workspace_idx: 2,
            is_output_visible: true,
            ..RawWorkspace::default()
        });
        let hidden = Workspace::from(RawWorkspace {
            tag_id: 3,
            workspace_idx: 3,
            ..RawWorkspace::default()
        });

        assert!(focused.visibility.is_focused());
        assert!(visible.visibility.is_visible());
        assert!(!hidden.visibility.is_visible());
    }

    #[cfg(feature = "workspaces")]
    #[test]
    fn decodes_layout_event_payload() {
        let event = decode_event(
            r#"{"triad":{"version":1,"event":"layout-state-changed","state":{"workspaces":[{"tag_id":2,"workspace_idx":2,"is_output_visible":true}]}}}"#,
        )
        .expect("event to parse");

        let EventMessage::Layout(layout) = event else {
            panic!("expected layout event");
        };

        assert_eq!(layout.workspaces.len(), 1);
        assert_eq!(layout.workspaces[0].tag_id, 2);
    }

    #[cfg(feature = "workspaces")]
    #[test]
    fn decodes_state_event_payload() {
        let event = decode_event(
            r#"{"triad":{"version":1,"event":"state-changed","state":{"layout":{"workspaces":[{"tag_id":3,"workspace_idx":3,"is_active":true}]}}}}"#,
        )
        .expect("event to parse");

        let EventMessage::State(state) = event else {
            panic!("expected state event");
        };

        assert_eq!(state.layout.workspaces.len(), 1);
        assert_eq!(state.layout.workspaces[0].tag_id, 3);
    }

    #[test]
    fn ignores_unknown_event_payload() {
        let event = decode_event(
            r#"{"triad":{"version":1,"event":"window-changed","window":{"id":1,"title":"Changed"}}}"#,
        )
        .expect("event to parse");

        let EventMessage::Unknown(event) = event else {
            panic!("expected unknown event");
        };

        assert_eq!(event.as_deref(), Some("window-changed"));
    }

    #[cfg(feature = "workspaces")]
    #[test]
    fn reports_workspace_urgent_updates() {
        let old_workspaces = vec![TriadWorkspace::from(RawWorkspace {
            tag_id: 1,
            workspace_idx: 1,
            is_urgent: false,
            ..RawWorkspace::default()
        })];
        let new_workspaces = vec![TriadWorkspace::from(RawWorkspace {
            tag_id: 1,
            workspace_idx: 1,
            is_urgent: true,
            ..RawWorkspace::default()
        })];

        let updates = workspace_updates(&old_workspaces, &new_workspaces);

        assert!(updates.iter().any(|update| {
            matches!(
                update,
                WorkspaceUpdate::Urgent {
                    id: 1,
                    urgent: true
                }
            )
        }));
    }
}
