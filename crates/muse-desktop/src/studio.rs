//! Native studio transcript and account-bound job state.
use crate::input::TextInput;
use gpui::{AppContext, Context, Entity, ScrollHandle};
use muse_core::{AccountId, BrowserIdentity};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};

pub const ADAPTER: &str = include_str!("../../../assets/muse-studio.js");
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Chat,
    Image,
    Video,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "聊天",
            Self::Image => "图片",
            Self::Video => "视频",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Image => "image",
            Self::Video => "video",
        }
    }
    /// Prompt when a reference image is attached (image-to-image / image-to-video).
    pub fn prompt_with_reference(self, prompt: &str, ratio: &str, duration: u8) -> String {
        match self {
            Self::Chat => prompt.to_owned(),
            Self::Image if ratio == "Auto" => {
                format!(
                    "请以附带的参考图为基础生成一张新图片，保持参考图的画幅比例。要求：{prompt}"
                )
            }
            Self::Image => {
                format!("请以附带的参考图为基础生成一张新图片，画幅比例 {ratio}。要求：{prompt}")
            }
            Self::Video if ratio == "Auto" => format!(
                "请以附带的参考图作为画面起点生成一个视频，沿用参考图的画幅比例，时长约 {duration} 秒。内容：{prompt}"
            ),
            Self::Video => format!(
                "请以附带的参考图作为画面起点生成一个视频，画幅比例 {ratio}，时长约 {duration} 秒。内容：{prompt}"
            ),
        }
    }
    pub fn prompt(self, prompt: &str, ratio: &str, duration: u8) -> String {
        match self {
            Self::Chat => prompt.to_owned(),
            Self::Image if ratio == "Auto" => {
                format!("请直接生成一张全新图片，不指定画幅比例。内容：{prompt}")
            }
            Self::Image => format!("请直接生成一张全新图片，画幅比例 {ratio}。内容：{prompt}"),
            Self::Video if ratio == "Auto" => format!(
                "请直接生成一个全新视频，不指定画幅比例，时长约 {duration} 秒。内容：{prompt}"
            ),
            Self::Video => format!(
                "请直接生成一个全新视频，画幅比例 {ratio}，时长约 {duration} 秒。内容：{prompt}"
            ),
        }
    }
}
/// A studio conversation. Each one keeps its own records and its own Muse
/// chat page per account, so a new conversation starts from a fresh thread.
#[derive(Serialize, Deserialize, Clone)]
pub struct Conversation {
    pub id: uuid::Uuid,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub title: String,
}
#[derive(Serialize, Deserialize, Default)]
struct ConversationFile {
    #[serde(default)]
    active: Option<uuid::Uuid>,
    #[serde(default)]
    conversations: Vec<Conversation>,
}
const CONVERSATIONS_FILE: &str = "studio-conversations.json";

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub id: uuid::Uuid,
    /// Owning conversation. Records saved before conversations existed load
    /// with the nil id and are moved into a single "早期记录" conversation.
    #[serde(default)]
    pub conversation: uuid::Uuid,
    pub account: AccountId,
    pub name: String,
    pub prompt: String,
    pub mode: Mode,
    #[serde(default = "default_ratio")]
    pub ratio: String,
    #[serde(default = "default_duration")]
    pub duration: u8,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub media_width: Option<u32>,
    #[serde(default)]
    pub media_height: Option<u32>,
    pub text: String,
    pub status: String,
    pub media: Option<std::string::String>,
    pub running: bool,
    #[serde(default)]
    pub source_tab: Option<muse_core::TabId>,
    #[serde(default)]
    pub source_url: Option<std::string::String>,
    #[serde(default)]
    pub media_original: bool,
    #[serde(default)]
    pub media_attachment: Option<std::string::String>,
    /// Reference image used for this record, relative to the data directory.
    #[serde(default)]
    pub reference: Option<std::string::String>,
}
fn default_ratio() -> String {
    "Auto".into()
}
fn default_duration() -> u8 {
    5
}
pub enum Phase {
    /// Sending the reference image to the page in 24 KB chunks, then attaching it.
    Uploading {
        data: std::sync::Arc<Vec<u8>>,
        offset: usize,
    },
    Preparing,
    Recovering {
        thumbnail: String,
    },
    Watching,
    Downloading {
        bytes: usize,
        total: usize,
        file: PathBuf,
    },
}
pub struct Job {
    pub id: uuid::Uuid,
    pub identity: Option<BrowserIdentity>,
    pub phase: Phase,
    pub started: Instant,
    pub last_poll: Instant,
    pub pending: bool,
    pub prompt: String,
    pub mode: Mode,
    pub recovering: bool,
    pub receipt: Option<MediaReceipt>,
}
pub struct Studio {
    pub input: Entity<TextInput>,
    pub mode: Mode,
    pub ratio: usize,
    pub duration: usize,
    pub viewer: Option<(
        crate::studio_image::ImageViewer,
        crate::motion::TransitionState,
    )>,
    pub selected: HashSet<AccountId>,
    /// Folder last chosen in the save dialog.
    pub save_dir: Option<PathBuf>,
    /// Reference image attached to the next prompt.
    pub reference: Option<crate::studio_reference::Reference>,
    /// A picture is being prepared in the background.
    pub reference_loading: bool,
    pub entries: Vec<Entry>,
    pub conversations: Vec<Conversation>,
    pub active: uuid::Uuid,
    pub conversations_open: bool,
    pub conversation_delete: Option<uuid::Uuid>,
    /// Own transition for switching conversations, so the account sidebar's
    /// selection does not replay its animation.
    pub conversation_transition: crate::motion::TransitionState,
    /// Records whose prompt is shown in full (otherwise three lines).
    pub expanded_prompts: HashSet<uuid::Uuid>,
    /// Results that arrived in this session; they "develop" into view once.
    pub fresh_media: HashMap<uuid::Uuid, Instant>,
    pub jobs: HashMap<AccountId, Job>,
    /// Muse chat page used by each (conversation, account) pair.
    pub sessions: HashMap<(uuid::Uuid, AccountId), BrowserIdentity>,
    pub scroll: ScrollHandle,
    pub notice: String,
    pub last_revision: u64,
}
impl Studio {
    pub fn new<T: 'static>(root: &Path, cx: &mut Context<T>) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("", cx);
            input.set_placeholder("描述你想生成的画面、风格和细节…");
            input.set_placeholder_color(0x86878E);
            input.set_font_size(14.);
            input
        });
        let mut entries = std::fs::metadata(root.join("studio-history.json"))
            .ok()
            .filter(|m| m.len() <= 8_000_000)
            .and_then(|_| std::fs::read(root.join("studio-history.json")).ok())
            .and_then(|b| serde_json::from_slice::<Vec<Entry>>(&b).ok())
            .unwrap_or_default();
        entries.truncate(200);
        for entry in &mut entries {
            if entry.running {
                entry.running = false;
                entry.status = "上次任务已中断".into();
            }
            if entry
                .media
                .as_ref()
                .is_some_and(|p| !safe_media_path(root, p).is_some_and(|p| p.is_file()))
            {
                entry.media = None;
            }
            update_media_dimensions(root, entry);
        }
        let saved = std::fs::metadata(root.join(CONVERSATIONS_FILE))
            .ok()
            .filter(|m| m.len() <= 1_000_000)
            .and_then(|_| std::fs::read(root.join(CONVERSATIONS_FILE)).ok())
            .and_then(|b| serde_json::from_slice::<ConversationFile>(&b).ok())
            .unwrap_or_default();
        let mut conversations = saved.conversations;
        conversations.retain(|conversation| !conversation.id.is_nil());
        let mut legacy: Option<uuid::Uuid> = None;
        for entry in &mut entries {
            if conversations.iter().any(|c| c.id == entry.conversation) {
                continue;
            }
            if entry.conversation.is_nil() {
                let id = *legacy.get_or_insert_with(uuid::Uuid::new_v4);
                entry.conversation = id;
            }
            if !conversations.iter().any(|c| c.id == entry.conversation) {
                conversations.push(Conversation {
                    id: entry.conversation,
                    created_at: entry.created_at,
                    title: if Some(entry.conversation) == legacy {
                        "早期记录".into()
                    } else {
                        String::new()
                    },
                });
            }
        }
        if conversations.is_empty() {
            conversations.push(Conversation {
                id: uuid::Uuid::new_v4(),
                created_at: crate::page_observer::now_ms(),
                title: String::new(),
            });
        }
        let scroll = ScrollHandle::default();
        scroll.scroll_to_bottom();
        let mut studio = Self {
            input,
            mode: Mode::Image,
            ratio: 0,
            duration: 0,
            viewer: None,
            selected: HashSet::new(),
            save_dir: None,
            reference: None,
            reference_loading: false,
            entries,
            conversations,
            active: uuid::Uuid::nil(),
            conversations_open: true,
            conversation_delete: None,
            conversation_transition: crate::motion::TransitionState::visible(),
            expanded_prompts: HashSet::new(),
            fresh_media: HashMap::new(),
            jobs: HashMap::new(),
            sessions: HashMap::new(),
            scroll,
            notice: String::new(),
            last_revision: 0,
        };
        let latest = studio.ordered_conversations()[0].id;
        let active = saved
            .active
            .filter(|id| studio.conversations.iter().any(|c| c.id == *id))
            .unwrap_or(latest);
        studio.active = active;
        studio
    }

    /// Latest activity: the newest record, or the creation time when empty.
    pub fn activity(&self, id: uuid::Uuid) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.conversation == id)
            .map(|entry| entry.created_at)
            .max()
            .or_else(|| {
                self.conversations
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.created_at)
            })
            .unwrap_or(0)
    }

    pub fn ordered_conversations(&self) -> Vec<&Conversation> {
        let mut list = self.conversations.iter().collect::<Vec<_>>();
        list.sort_by_key(|c| std::cmp::Reverse(self.activity(c.id)));
        list
    }

    pub fn record_count(&self, id: uuid::Uuid) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.conversation == id && entry.mode != Mode::Chat)
            .count()
    }

    pub fn is_running(&self, id: uuid::Uuid) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.conversation == id && entry.running)
    }

    /// Manual title, else the first prompt (whole characters only), else 新会话.
    pub fn title(&self, id: uuid::Uuid) -> String {
        if let Some(title) = self
            .conversations
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.trim())
            .filter(|title| !title.is_empty())
        {
            return title.to_owned();
        }
        self.entries
            .iter()
            .filter(|entry| entry.conversation == id && entry.mode != Mode::Chat)
            .min_by_key(|entry| entry.created_at)
            .map(|entry| {
                let text = entry
                    .prompt
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut short = text.chars().take(24).collect::<String>();
                if text.chars().count() > 24 {
                    short.push('…');
                }
                short
            })
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "新会话".into())
    }

    /// Starts a new conversation; an empty current one is reused.
    pub fn new_conversation(&mut self) -> uuid::Uuid {
        if self.record_count(self.active) == 0
            && self.conversations.iter().any(|c| c.id == self.active)
        {
            return self.active;
        }
        let id = uuid::Uuid::new_v4();
        self.conversations.push(Conversation {
            id,
            created_at: crate::page_observer::now_ms(),
            title: String::new(),
        });
        self.active = id;
        self.scroll.scroll_to_bottom();
        id
    }

    pub fn select_conversation(&mut self, id: uuid::Uuid) {
        if self.conversations.iter().any(|c| c.id == id) {
            self.active = id;
            self.conversation_delete = None;
            self.scroll.scroll_to_bottom();
        }
    }

    /// Removes a conversation and its records. Refused while a task is running.
    pub fn remove_conversation(&mut self, id: uuid::Uuid) -> bool {
        if self.is_running(id) {
            return false;
        }
        self.entries.retain(|entry| entry.conversation != id);
        self.conversations.retain(|c| c.id != id);
        self.sessions
            .retain(|(conversation, _), _| *conversation != id);
        if self.conversations.is_empty() {
            self.conversations.push(Conversation {
                id: uuid::Uuid::new_v4(),
                created_at: crate::page_observer::now_ms(),
                title: String::new(),
            });
        }
        if self.active == id || !self.conversations.iter().any(|c| c.id == self.active) {
            let latest = self.ordered_conversations()[0].id;
            self.active = latest;
            self.scroll.scroll_to_bottom();
        }
        self.conversation_delete = None;
        true
    }
    pub fn save(&self, root: &Path) {
        if let Ok(bytes) = serde_json::to_vec(&ConversationFile {
            active: Some(self.active),
            conversations: self.conversations.clone(),
        }) {
            let _ = crate::account_cache::write(root, CONVERSATIONS_FILE, &bytes);
        }
        let mut start = self.entries.len().saturating_sub(200);
        while start < self.entries.len() {
            let Ok(bytes) = serde_json::to_vec(&self.entries[start..]) else {
                return;
            };
            if bytes.len() <= 8_000_000 {
                let _ = crate::account_cache::write(root, "studio-history.json", &bytes);
                return;
            }
            start += 1;
        }
    }
    pub fn entry_mut(&mut self, id: uuid::Uuid) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }
}
#[derive(Deserialize)]
pub struct Reply {
    pub id: Option<uuid::Uuid>,
    pub state: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub bytes: usize,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub eof: bool,
    #[serde(flatten)]
    pub receipt: MediaReceipt,
}
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MediaReceipt {
    pub original: bool,
    pub width: u32,
    pub height: u32,
    pub attachment: String,
}
pub fn decode(raw: &str) -> Option<Reply> {
    if raw.len() > 1_000_000 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if let Some(s) = value.as_str() {
        serde_json::from_str(s).ok()
    } else {
        serde_json::from_value(value).ok()
    }
}
pub fn safe_media_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if !relative.starts_with("media/")
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(path))
}
pub const IMAGE_RATIOS: [&str; 6] = ["Auto", "1:1", "16:9", "9:16", "4:3", "3:4"];
pub const VIDEO_RATIOS: [&str; 4] = ["Auto", "1:1", "16:9", "9:16"];
pub fn ratio_options(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Image => &IMAGE_RATIOS,
        Mode::Video => &VIDEO_RATIOS,
        Mode::Chat => &[],
    }
}
pub fn ratio_value(mode: Mode, index: usize) -> &'static str {
    ratio_options(mode).get(index).copied().unwrap_or("Auto")
}
pub const DURATIONS: [u8; 4] = [5, 6, 8, 10];

pub fn update_media_dimensions(root: &Path, entry: &mut Entry) {
    if entry.mode == Mode::Image
        && let Some(path) = entry
            .media
            .as_ref()
            .and_then(|path| safe_media_path(root, path))
        && let Ok((width, height)) = image::image_dimensions(path)
        && width > 0
        && height > 0
    {
        entry.media_width = Some(width);
        entry.media_height = Some(height);
    }
}
