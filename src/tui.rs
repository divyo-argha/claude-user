use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::io::{stdout, Stdout};

use crate::aliases;
use crate::mappings;
use crate::profiles;

type CuserTerminal = Terminal<CrosstermBackend<Stdout>>;

const NEW_PROFILE: &str = "+ Add new account / profile (press 'n')";
const ADD_TOKEN: &str = "+ Add via OAuth token or API key (press 't')";
const IMPORT_DEFAULT: &str = "+ Import ~/.claude account (press 'i')";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Default,
    QuotaAvailable,
    SoonestReset,
    Alphabetical,
}

pub enum PickResult {
    Existing(String),
    New(String),
    Import(String),
    RunSession(String),
}

enum Action {
    New,
    Import,
    Rename { old: String },
    Alias { profile: String },
}

enum StatusMessage {
    Error(String),
    Info(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenStep {
    EnteringToken,
    EnteringName,
}

enum Mode {
    Picking,
    Search { query: String },
    Naming { action: Action, buffer: String },
    AddToken {
        step: TokenStep,
        token_buf: String,
        name_buf: String,
    },
    ConfirmDelete { name: String },
    Help,
}

enum Item {
    Profile {
        name: String,
        alias: Option<String>,
        email: Option<String>,
        org_name: Option<String>,
        is_current: bool,
        is_disabled: bool,
        is_mapped: bool,
        usage: Option<Box<crate::usage::AccountUsage>>,
    },
    ImportDefault,
    NewProfile,
    AddToken,
}

pub fn run_picker() -> Result<Option<PickResult>> {
    let (items, current_profile, current_idx) = build_items()?;

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal: CuserTerminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, items, current_profile, current_idx);

    disable_raw_mode().ok();
    stdout().execute(LeaveAlternateScreen).ok();

    result
}

fn build_items() -> Result<(Vec<Item>, Option<String>, Option<usize>)> {
    let current = profiles::current_profile()?;
    let cwd = std::env::current_dir().ok();
    let mapped_profile = cwd.as_ref().and_then(|p| {
        mappings::resolve_mapping(p).ok().flatten().map(|(name, _)| name)
    });
    let aliases_map = aliases::load_aliases().unwrap_or_default();

    let mut items = Vec::new();
    let mut current_idx = None;

    for name in profiles::list_profiles()? {
        let is_current = current.as_deref() == Some(&name);
        let is_mapped = mapped_profile.as_deref() == Some(&name);
        let is_disabled = profiles::is_profile_disabled(&name).unwrap_or(false);

        if is_current {
            current_idx = Some(items.len());
        }
        let info = profiles::get_profile_info(&name).unwrap_or(profiles::ProfileInfo {
            name: name.clone(),
            email: None,
            org_name: None,
            is_disabled,
        });

        let my_aliases: Vec<&str> = aliases_map
            .iter()
            .filter(|(_, target)| target.as_str() == name)
            .map(|(a, _)| a.as_str())
            .collect();
        let primary_alias = my_aliases.first().map(|s| s.to_string());

        let usage_data = crate::usage::get_profile_usage(&name, false).ok().map(Box::new);
        items.push(Item::Profile {
            name,
            alias: primary_alias,
            email: info.email,
            org_name: info.org_name,
            is_current,
            is_disabled,
            is_mapped,
            usage: usage_data,
        });
    }

    if current_idx.is_none() && mapped_profile.is_some() {
        for (i, it) in items.iter().enumerate() {
            if let Item::Profile { is_mapped: true, .. } = it {
                current_idx = Some(i);
                break;
            }
        }
    }

    if profiles::can_import()? {
        items.push(Item::ImportDefault);
    }
    items.push(Item::NewProfile);
    items.push(Item::AddToken);
    Ok((items, current, current_idx))
}

fn get_profile_max_pct(item: &Item) -> f64 {
    match item {
        Item::Profile { usage: Some(u), .. } => {
            let p5 = u.five_hour.as_ref().map(|w| w.pct).unwrap_or(0.0);
            let p7 = u.seven_day.as_ref().map(|w| w.pct).unwrap_or(0.0);
            p5.max(p7)
        }
        _ => 999.0,
    }
}

fn get_profile_earliest_reset(item: &Item) -> Option<String> {
    match item {
        Item::Profile { usage: Some(u), .. } => {
            let r5 = u.five_hour.as_ref().and_then(|w| w.resets_at.clone());
            let r7 = u.seven_day.as_ref().and_then(|w| w.resets_at.clone());
            match (r5, r7) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        }
        _ => None,
    }
}

fn filter_and_sort_items<'a>(
    items: &'a [Item],
    query: &str,
    sort_order: SortOrder,
) -> Vec<(usize, &'a Item)> {
    let q = query.trim().to_lowercase();
    let mut list: Vec<(usize, &'a Item)> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            if q.is_empty() {
                return true;
            }
            match it {
                Item::Profile {
                    name,
                    alias,
                    email,
                    org_name,
                    ..
                } => {
                    name.to_lowercase().contains(&q)
                        || alias
                            .as_ref()
                            .map(|a| a.to_lowercase().contains(&q))
                            .unwrap_or(false)
                        || email
                            .as_ref()
                            .map(|e| e.to_lowercase().contains(&q))
                            .unwrap_or(false)
                        || org_name
                            .as_ref()
                            .map(|o| o.to_lowercase().contains(&q))
                            .unwrap_or(false)
                }
                Item::ImportDefault => "import default account".contains(&q),
                Item::NewProfile => "new add account profile".contains(&q),
                Item::AddToken => "token api key sk-ant".contains(&q),
            }
        })
        .collect();

    list.sort_by(|(_, a), (_, b)| {
        let a_is_action = matches!(a, Item::ImportDefault | Item::NewProfile | Item::AddToken);
        let b_is_action = matches!(b, Item::ImportDefault | Item::NewProfile | Item::AddToken);
        if a_is_action != b_is_action {
            return a_is_action.cmp(&b_is_action);
        }
        if a_is_action {
            return std::cmp::Ordering::Equal;
        }

        match sort_order {
            SortOrder::Default => std::cmp::Ordering::Equal,
            SortOrder::Alphabetical => {
                let name_a = match a {
                    Item::Profile { name, .. } => name.to_lowercase(),
                    _ => String::new(),
                };
                let name_b = match b {
                    Item::Profile { name, .. } => name.to_lowercase(),
                    _ => String::new(),
                };
                name_a.cmp(&name_b)
            }
            SortOrder::QuotaAvailable => {
                let pct_a = get_profile_max_pct(a);
                let pct_b = get_profile_max_pct(b);
                pct_a.partial_cmp(&pct_b).unwrap_or(std::cmp::Ordering::Equal)
            }
            SortOrder::SoonestReset => {
                let reset_a = get_profile_earliest_reset(a);
                let reset_b = get_profile_earliest_reset(b);
                match (reset_a, reset_b) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            }
        }
    });

    list
}

fn selected_profile_name(visible: &[(usize, &Item)], state: &ListState) -> Option<String> {
    match visible.get(state.selected().unwrap_or(0)) {
        Some((_, Item::Profile { name, .. })) => Some(name.clone()),
        _ => None,
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn event_loop(
    terminal: &mut CuserTerminal,
    mut items: Vec<Item>,
    mut current_profile: Option<String>,
    current_idx: Option<usize>,
) -> Result<Option<PickResult>> {
    let mut list_state = ListState::default();
    list_state.select(Some(current_idx.unwrap_or(0)));
    let mut mode = Mode::Picking;
    let mut status: Option<StatusMessage> = None;
    let mut search_query = String::new();
    let mut sort_order = SortOrder::Default;

    loop {
        let visible = filter_and_sort_items(&items, &search_query, sort_order);
        if let Some(sel) = list_state.selected() {
            if visible.is_empty() {
                list_state.select(None);
            } else if sel >= visible.len() {
                list_state.select(Some(visible.len().saturating_sub(1)));
            }
        } else if !visible.is_empty() {
            list_state.select(Some(0));
        }

        terminal.draw(|f| {
            let ctx = DrawContext {
                visible: &visible,
                total_items_count: items.len(),
                mode: &mode,
                status: &status,
                current_profile: &current_profile,
                search_query: &search_query,
                sort_order,
            };
            draw(f, &mut list_state, &ctx);
        })?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(None);
        }

        match &mut mode {
            Mode::Help => match key.code {
                KeyCode::Esc
                | KeyCode::Char('?')
                | KeyCode::Char('h')
                | KeyCode::Char('q')
                | KeyCode::Enter => {
                    mode = Mode::Picking;
                    status = None;
                }
                _ => {}
            },
            Mode::Search { query } => match key.code {
                KeyCode::Esc => {
                    search_query.clear();
                    mode = Mode::Picking;
                    status = Some(StatusMessage::Info("Search cleared.".to_string()));
                }
                KeyCode::Enter => {
                    mode = Mode::Picking;
                    status = None;
                }
                KeyCode::Backspace => {
                    query.pop();
                    search_query = query.clone();
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    search_query = query.clone();
                }
                KeyCode::Up | KeyCode::BackTab => {
                    let len = visible.len();
                    if len > 0 {
                        let i = list_state.selected().unwrap_or(0);
                        if i == 0 {
                            list_state.select(Some(len.saturating_sub(1)));
                        } else {
                            list_state.select(Some(i - 1));
                        }
                    }
                }
                KeyCode::Down | KeyCode::Tab => {
                    let len = visible.len();
                    if len > 0 {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < len {
                            list_state.select(Some(i + 1));
                        } else {
                            list_state.select(Some(0));
                        }
                    }
                }
                _ => {}
            },
            Mode::AddToken {
                step,
                token_buf,
                name_buf,
            } => match key.code {
                KeyCode::Esc => {
                    mode = Mode::Picking;
                    status = None;
                }
                KeyCode::Backspace => match step {
                    TokenStep::EnteringToken => {
                        token_buf.pop();
                    }
                    TokenStep::EnteringName => {
                        name_buf.pop();
                    }
                },
                KeyCode::Char(c) => match step {
                    TokenStep::EnteringToken => {
                        token_buf.push(c);
                    }
                    TokenStep::EnteringName => {
                        name_buf.push(c);
                    }
                },
                KeyCode::Enter => match step {
                    TokenStep::EnteringToken => {
                        let token = token_buf.trim().to_string();
                        let is_oauth = token.starts_with("sk-ant-oat01-")
                            || token.starts_with("sk-ant-oat");
                        let is_api_key = token.starts_with("sk-ant-api");
                        if !is_oauth && !is_api_key {
                            status = Some(StatusMessage::Error(
                                "Invalid format: must start with 'sk-ant-oat...' (OAuth token) or 'sk-ant-api...' (API key)".to_string(),
                            ));
                        } else {
                            let prefix = if is_oauth { "token" } else { "api" };
                            let mut idx = 1;
                            while profiles::profile_exists(&format!("{prefix}-{idx}"))
                                .unwrap_or(false)
                            {
                                idx += 1;
                            }
                            *step = TokenStep::EnteringName;
                            *name_buf = format!("{prefix}-{idx}");
                            status = None;
                        }
                    }
                    TokenStep::EnteringName => {
                        let chosen_name = name_buf.trim().to_string();
                        if chosen_name.is_empty() {
                            status = Some(StatusMessage::Error(
                                "Profile name cannot be empty".to_string(),
                            ));
                        } else if let Err(e) = profiles::validate_profile_name(&chosen_name) {
                            status = Some(StatusMessage::Error(e.to_string()));
                        } else if profiles::profile_exists(&chosen_name).unwrap_or(false) {
                            status = Some(StatusMessage::Error(format!(
                                "Profile \"{chosen_name}\" already exists"
                            )));
                        } else {
                            match crate::token::add_token(token_buf, &chosen_name, None, None) {
                                Ok(()) => {
                                    let (new_items, new_current, _) = build_items()?;
                                    items = new_items;
                                    current_profile = new_current;
                                    status = Some(StatusMessage::Info(format!(
                                        "Registered profile \"{chosen_name}\" from token!"
                                    )));
                                    mode = Mode::Picking;
                                }
                                Err(e) => {
                                    status = Some(StatusMessage::Error(format!(
                                        "Failed to add token: {e}"
                                    )));
                                    mode = Mode::Picking;
                                }
                            }
                        }
                    }
                },
                _ => {}
            },
            Mode::Picking => match key.code {
                KeyCode::Char('q') => return Ok(None),
                KeyCode::Esc => {
                    if !search_query.is_empty() {
                        search_query.clear();
                        status = Some(StatusMessage::Info("Cleared filter.".to_string()));
                    } else {
                        return Ok(None);
                    }
                }
                KeyCode::Char('/') => {
                    mode = Mode::Search {
                        query: search_query.clone(),
                    };
                    status = None;
                }
                KeyCode::Char('?') | KeyCode::Char('h') => {
                    mode = Mode::Help;
                    status = None;
                }
                KeyCode::Char('o') | KeyCode::Char('O') => {
                    sort_order = match sort_order {
                        SortOrder::Default => SortOrder::QuotaAvailable,
                        SortOrder::QuotaAvailable => SortOrder::SoonestReset,
                        SortOrder::SoonestReset => SortOrder::Alphabetical,
                        SortOrder::Alphabetical => SortOrder::Default,
                    };
                    let sort_name = match sort_order {
                        SortOrder::Default => "Default",
                        SortOrder::QuotaAvailable => "Quota: Most Available",
                        SortOrder::SoonestReset => "Reset: Soonest First",
                        SortOrder::Alphabetical => "Alphabetical (A-Z)",
                    };
                    status = Some(StatusMessage::Info(format!("Sorted by: {sort_name}")));
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    mode = Mode::AddToken {
                        step: TokenStep::EnteringToken,
                        token_buf: String::new(),
                        name_buf: String::new(),
                    };
                    status = None;
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        return Ok(Some(PickResult::RunSession(name)));
                    } else {
                        status = Some(StatusMessage::Info(
                            "Select an existing profile to run in session mode.".to_string(),
                        ));
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('+') => {
                    mode = Mode::Naming {
                        action: Action::New,
                        buffer: String::new(),
                    };
                    status = None;
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    if profiles::can_import().unwrap_or(false) {
                        mode = Mode::Naming {
                            action: Action::Import,
                            buffer: String::new(),
                        };
                        status = None;
                    } else {
                        status = Some(StatusMessage::Info(
                            "No default ~/.claude directory available to import.".to_string(),
                        ));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    let len = visible.len();
                    if len > 0 {
                        let i = list_state.selected().unwrap_or(0);
                        if i == 0 {
                            list_state.select(Some(len.saturating_sub(1)));
                        } else {
                            list_state.select(Some(i - 1));
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    let len = visible.len();
                    if len > 0 {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < len {
                            list_state.select(Some(i + 1));
                        } else {
                            list_state.select(Some(0));
                        }
                    }
                }
                KeyCode::Home => {
                    if !visible.is_empty() {
                        list_state.select(Some(0));
                    }
                }
                KeyCode::End => {
                    let len = visible.len();
                    if len > 0 {
                        list_state.select(Some(len.saturating_sub(1)));
                    }
                }
                KeyCode::PageUp => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(5)));
                }
                KeyCode::PageDown => {
                    let len = visible.len();
                    if len > 0 {
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some((i + 5).min(len.saturating_sub(1))));
                    }
                }
                KeyCode::Char('s') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        match profiles::activate_profile(&name) {
                            Ok(()) => {
                                current_profile = Some(name.clone());
                                let sel = list_state.selected();
                                let (new_items, _, _) = build_items()?;
                                items = new_items;
                                list_state.select(sel);
                                status = Some(StatusMessage::Info(format!(
                                    "Switched active profile to \"{name}\". Run `claude` to use it."
                                )));
                            }
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        }
                    }
                }
                KeyCode::Char('a') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        let existing = aliases::aliases_for_profile(&name).unwrap_or_default();
                        let initial = existing.first().cloned().unwrap_or_default();
                        mode = Mode::Naming {
                            action: Action::Alias { profile: name },
                            buffer: initial,
                        };
                        status = None;
                    }
                }
                KeyCode::Char('m') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state)
                        && let Ok(cwd) = std::env::current_dir()
                    {
                        let is_mapped_to_selected = mappings::resolve_mapping(&cwd)
                            .ok()
                            .flatten()
                            .map(|(p, _)| p == name)
                            .unwrap_or(false);

                        let cwd_str = cwd.to_string_lossy();
                        if is_mapped_to_selected {
                            match mappings::remove_mapping(Some(cwd_str.as_ref())) {
                                Ok((path, Some(prev))) => {
                                    let sel = list_state.selected();
                                    let (new_items, _, _) = build_items()?;
                                    items = new_items;
                                    list_state.select(sel);
                                    status = Some(StatusMessage::Info(format!(
                                        "Unmapped directory \"{}\" from \"{prev}\".",
                                        path.display()
                                    )));
                                }
                                Ok((_, None)) => {}
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        } else {
                            match mappings::add_mapping(&name, Some(cwd_str.as_ref())) {
                                Ok(path) => {
                                    let sel = list_state.selected();
                                    let (new_items, _, _) = build_items()?;
                                    items = new_items;
                                    list_state.select(sel);
                                    status = Some(StatusMessage::Info(format!(
                                        "Mapped \"{}\" -> \"{name}\".",
                                        path.display()
                                    )));
                                }
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        }
                    }
                }
                KeyCode::Char('u') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        match crate::usage::get_profile_usage(&name, true) {
                            Ok(_) => {
                                let sel = list_state.selected();
                                let (new_items, _, _) = build_items()?;
                                items = new_items;
                                list_state.select(sel);
                                status = Some(StatusMessage::Info(format!(
                                    "Refreshed quota for \"{name}\"."
                                )));
                            }
                            Err(e) => {
                                status = Some(StatusMessage::Error(format!("Usage error: {e}")));
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        mode = Mode::ConfirmDelete { name };
                        status = None;
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        mode = Mode::Naming {
                            action: Action::Rename { old: name.clone() },
                            buffer: name,
                        };
                        status = None;
                    }
                }
                KeyCode::Char('e') => {
                    if let Some(name) = selected_profile_name(&visible, &list_state) {
                        let is_disabled = profiles::is_profile_disabled(&name).unwrap_or(false);
                        match profiles::set_profile_disabled(&name, !is_disabled) {
                            Ok(()) => {
                                let sel = list_state.selected();
                                let (new_items, new_current, _) = build_items()?;
                                items = new_items;
                                current_profile = new_current;
                                list_state.select(sel);
                                let state_str = if !is_disabled { "disabled" } else { "enabled" };
                                status = Some(StatusMessage::Info(format!(
                                    "Profile \"{name}\" is now {state_str}."
                                )));
                            }
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some((_, selected)) = visible.get(list_state.selected().unwrap_or(0)) {
                        match selected {
                            Item::NewProfile => {
                                mode = Mode::Naming {
                                    action: Action::New,
                                    buffer: String::new(),
                                };
                            }
                            Item::AddToken => {
                                mode = Mode::AddToken {
                                    step: TokenStep::EnteringToken,
                                    token_buf: String::new(),
                                    name_buf: String::new(),
                                };
                            }
                            Item::ImportDefault => {
                                mode = Mode::Naming {
                                    action: Action::Import,
                                    buffer: String::new(),
                                };
                            }
                            Item::Profile { name, .. } => {
                                return Ok(Some(PickResult::Existing(name.clone())));
                            }
                        }
                        status = None;
                    }
                }
                _ => {}
            },
            Mode::Naming { action, buffer } => match key.code {
                KeyCode::Esc => {
                    mode = Mode::Picking;
                    status = None;
                }
                KeyCode::Enter => {
                    let text = buffer.trim().to_string();
                    match action {
                        Action::Alias { profile } => {
                            let profile = profile.clone();
                            if text.is_empty() {
                                let _ = aliases::remove_aliases_for_profile(&profile);
                                let (new_items, new_current, _) = build_items()?;
                                items = new_items;
                                current_profile = new_current;
                                mode = Mode::Picking;
                                status = Some(StatusMessage::Info(format!(
                                    "Cleared aliases for \"{profile}\"."
                                )));
                            } else {
                                match aliases::set_alias_quiet(&profile, &text) {
                                    Ok(()) => {
                                        let (new_items, new_current, _) = build_items()?;
                                        items = new_items;
                                        current_profile = new_current;
                                        mode = Mode::Picking;
                                        status = Some(StatusMessage::Info(format!(
                                            "Aliased \"{text}\" -> \"{profile}\"."
                                        )));
                                    }
                                    Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                                }
                            }
                        }
                        Action::Rename { old } => {
                            let old = old.clone();
                            if text.is_empty() {
                                status = Some(StatusMessage::Error(
                                    "profile name cannot be empty".to_string(),
                                ));
                            } else if text == old {
                                mode = Mode::Picking;
                                status = None;
                            } else {
                                match profiles::rename_profile(&old, &text) {
                                    Ok(()) => {
                                        let (new_items, new_current, new_idx) = build_items()?;
                                        items = new_items;
                                        current_profile = new_current;
                                        list_state.select(Some(new_idx.unwrap_or(0)));
                                        mode = Mode::Picking;
                                        status = Some(StatusMessage::Info(format!(
                                            "Renamed profile to \"{text}\"."
                                        )));
                                    }
                                    Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                                }
                            }
                        }
                        Action::New => match profiles::validate_profile_name(&text) {
                            Ok(()) => return Ok(Some(PickResult::New(text))),
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        },
                        Action::Import => match profiles::validate_profile_name(&text) {
                            Ok(()) => return Ok(Some(PickResult::Import(text))),
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        },
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c) => buffer.push(c),
                _ => {}
            },
            Mode::ConfirmDelete { name } => {
                let name = name.clone();
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => match profiles::remove_profile(&name) {
                        Ok(()) => {
                            let (new_items, new_current, _) = build_items()?;
                            items = new_items;
                            current_profile = new_current;
                            let idx = list_state
                                .selected()
                                .unwrap_or(0)
                                .min(items.len().saturating_sub(1));
                            list_state.select(Some(idx));
                            mode = Mode::Picking;
                            status = Some(StatusMessage::Info(format!(
                                "Deleted profile \"{name}\"."
                            )));
                        }
                        Err(e) => {
                            mode = Mode::Picking;
                            status = Some(StatusMessage::Error(e.to_string()));
                        }
                    },
                    _ => {
                        mode = Mode::Picking;
                        status = None;
                    }
                }
            }
        }
    }
}

struct DrawContext<'a> {
    visible: &'a [(usize, &'a Item)],
    total_items_count: usize,
    mode: &'a Mode,
    status: &'a Option<StatusMessage>,
    current_profile: &'a Option<String>,
    search_query: &'a str,
    sort_order: SortOrder,
}

fn draw(f: &mut Frame, state: &mut ListState, ctx: &DrawContext) {
    let DrawContext {
        visible,
        total_items_count,
        mode,
        status,
        current_profile,
        search_query,
        sort_order,
    } = *ctx;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Big Aesthetic Unicode Logo & System Header
            Constraint::Min(8),    // Side-by-Side Master-Detail Body
            Constraint::Length(4), // Bottom Status & Keybindings Panel
        ])
        .split(f.area());

    let logo_color_1 = Color::Rgb(217, 119, 87); // Claude Terracotta / Coral
    let logo_color_2 = Color::Rgb(148, 163, 184); // Slate
    let border_color = Color::Rgb(71, 85, 105);
    let accent_color = Color::Rgb(168, 85, 247); // Purple
    let active_color = Color::Rgb(74, 222, 128); // Green
    let muted_text = Color::Rgb(148, 163, 184);

    // 1. Big Aesthetic Unicode Header
    let header_chunks = if f.area().width >= 86 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(48),
                Constraint::Min(30),
            ])
            .split(chunks[0])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(100),
                Constraint::Length(0),
            ])
            .split(chunks[0])
    };

    let logo = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::styled("░░░░░█░█░█▀▀░█▀▀░█▀▄", Style::default().fg(logo_color_2).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ░█░░░█░░░█▀█░█░█░█░█░█▀▀", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::styled("░▄▄▄░█░█░▀▀█░█▀▀░█▀▄", Style::default().fg(logo_color_2).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::styled("░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀", Style::default().fg(logo_color_2).add_modifier(Modifier::BOLD)),
        ]),
    ]);
    f.render_widget(logo, header_chunks[0]);

    if f.area().width >= 86 {
        let meta_lines = vec![
            Line::from(vec![
                Span::styled("✦ CLAUDE-USER ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
                Span::styled("v0.3.0", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  ─  Multi-Account & Quota Manager", Style::default().fg(Color::Rgb(148, 163, 184))),
            ]),
            Line::from(vec![
                Span::styled("Active Profile: ", Style::default().fg(Color::Rgb(148, 163, 184))),
                match current_profile {
                    Some(curr) => Span::styled(
                        format!("● {curr}"),
                        Style::default().fg(active_color).add_modifier(Modifier::BOLD),
                    ),
                    None => Span::styled("(none)", Style::default().fg(Color::DarkGray)),
                },
            ]),
            Line::from(vec![
                Span::styled("Quick tip: ", Style::default().fg(Color::Rgb(100, 116, 139))),
                Span::styled("Press ", Style::default().fg(Color::Rgb(148, 163, 184))),
                Span::styled("[?]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" help  •  ", Style::default().fg(Color::Rgb(148, 163, 184))),
                Span::styled("[/]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" filter  •  ", Style::default().fg(Color::Rgb(148, 163, 184))),
                Span::styled("[o]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" sort  •  ", Style::default().fg(Color::Rgb(148, 163, 184))),
                Span::styled("[x]", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                Span::styled(" session", Style::default().fg(Color::Rgb(148, 163, 184))),
            ]),
        ];
        f.render_widget(Paragraph::new(meta_lines), header_chunks[1]);
    }

    // 2. Responsive Main Area Layout
    let is_wide = f.area().width >= 80;
    let main_chunks = if is_wide {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(42),
                Constraint::Percentage(58),
            ])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ])
            .split(chunks[1])
    };

    // Left Column: PROFILES list
    let list_items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(v_idx, (_, item))| {
            let is_selected = state.selected() == Some(v_idx);
            let prefix = if is_selected { " ▶ " } else { "   " };

            match item {
                Item::Profile {
                    name,
                    alias,
                    is_current,
                    is_disabled,
                    is_mapped,
                    usage,
                    ..
                } => {
                    let style = if is_selected {
                        Style::default().fg(accent_color).add_modifier(Modifier::BOLD)
                    } else if *is_disabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let mut spans = vec![
                        Span::styled(prefix, Style::default().fg(accent_color)),
                        Span::styled(name.clone(), style),
                    ];

                    if let Some(a) = alias {
                        spans.push(Span::styled(format!(" (@{a})"), Style::default().fg(Color::Yellow)));
                    }

                    if *is_current {
                        spans.push(Span::styled(
                            " ● active",
                            Style::default().fg(active_color).add_modifier(Modifier::BOLD),
                        ));
                    }

                    if *is_mapped {
                        spans.push(Span::styled(
                            " ★ mapped",
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        ));
                    }

                    if *is_disabled {
                        spans.push(Span::styled(
                            " [disabled]",
                            Style::default().fg(Color::DarkGray),
                        ));
                    }

                    if let Some(u) = usage {
                        match u.status {
                            crate::usage::UsageStatus::Ok => {
                                if let Some(h5) = &u.five_hour {
                                    let col = crate::usage::progress_color_ratatui(h5.pct);
                                    spans.push(Span::styled(
                                        format!("  {:.0}%", h5.pct),
                                        Style::default().fg(col).add_modifier(Modifier::BOLD),
                                    ));
                                }
                            }
                            crate::usage::UsageStatus::RateLimited => {
                                spans.push(Span::styled(" [429]", Style::default().fg(Color::Yellow)));
                            }
                            crate::usage::UsageStatus::TokenExpired => {
                                spans.push(Span::styled(" [expired]", Style::default().fg(Color::Red)));
                            }
                            _ => {}
                        }
                    }

                    ListItem::new(Line::from(spans))
                }
                Item::AddToken => {
                    let style = if is_selected {
                        Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(125, 211, 252))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(56, 189, 248))),
                        Span::styled(ADD_TOKEN, style),
                    ]))
                }
                Item::ImportDefault => {
                    let style = if is_selected {
                        Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(253, 224, 71))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(250, 204, 21))),
                        Span::styled(IMPORT_DEFAULT, style),
                    ]))
                }
                Item::NewProfile => {
                    let style = if is_selected {
                        Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(134, 239, 172))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(74, 222, 128))),
                        Span::styled(NEW_PROFILE, style),
                    ]))
                }
            }
        })
        .collect();

    let filter_badge = if !search_query.is_empty() {
        format!(" [Filter: \"{search_query}\" ({}/{})] ", visible.len(), total_items_count)
    } else {
        String::new()
    };
    let sort_badge = match sort_order {
        SortOrder::Default => "",
        SortOrder::QuotaAvailable => " [Sort: Quota (o)]",
        SortOrder::SoonestReset => " [Sort: Soonest (o)]",
        SortOrder::Alphabetical => " [Sort: A-Z (o)]",
    };
    let profiles_title = format!(" PROFILES{filter_badge}{sort_badge} ");

    let list_border = if matches!(mode, Mode::Search { .. }) {
        Color::Rgb(250, 204, 21)
    } else {
        border_color
    };

    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(list_border))
            .title(Span::styled(
                profiles_title,
                Style::default().fg(accent_color).add_modifier(Modifier::BOLD),
            )),
    );
    f.render_stateful_widget(list, main_chunks[0], state);

    // Right Column: LIVE QUOTA & RATE LIMITS
    let bar_width = (main_chunks[1].width as usize).saturating_sub(26).clamp(8, 22);
    let right_widget = match visible.get(state.selected().unwrap_or(0)) {
        Some((_, Item::Profile {
            name,
            alias,
            email,
            org_name,
            is_current,
            is_disabled,
            is_mapped,
            usage,
        })) => {
            let mut lines = Vec::new();

            // Account info
            let mut name_spans = vec![
                Span::styled("Profile: ", Style::default().fg(muted_text)),
                Span::styled(name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ];
            if let Some(a) = alias {
                name_spans.push(Span::styled(format!("  (@{a})"), Style::default().fg(Color::Yellow)));
            }
            if *is_current {
                name_spans.push(Span::styled("  ● Currently Active", Style::default().fg(active_color).add_modifier(Modifier::BOLD)));
            }
            lines.push(Line::from(name_spans));

            let acc_str = match (email, org_name) {
                (Some(e), Some(o)) => format!("{e}  •  {o}"),
                (Some(e), None) => e.clone(),
                (None, Some(o)) => o.clone(),
                (None, None) => "No email/org metadata cached".to_string(),
            };
            lines.push(Line::from(vec![
                Span::styled("Account: ", Style::default().fg(muted_text)),
                Span::styled(acc_str, Style::default().fg(Color::Rgb(226, 232, 240))),
            ]));

            if *is_mapped {
                lines.push(Line::from(vec![
                    Span::styled("Directory: ", Style::default().fg(muted_text)),
                    Span::styled("Mapped to current working directory", Style::default().fg(Color::Cyan)),
                ]));
            }
            if *is_disabled {
                lines.push(Line::from(vec![
                    Span::styled("State: ", Style::default().fg(muted_text)),
                    Span::styled("Disabled (excluded from rotation)", Style::default().fg(Color::DarkGray)),
                ]));
            }

            lines.push(Line::raw(""));

            match usage {
                Some(u) => {
                    let has_limits = u.five_hour.is_some() || u.seven_day.is_some() || u.spend.is_some() || !u.models.is_empty();

                    if let Some(h5) = &u.five_hour {
                        lines.push(Line::from(vec![
                            Span::styled("5-Hour Session Limit: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(h5.pct, bar_width));
                        if let Some(rst) = &h5.countdown {
                            b_spans.push(Span::styled(format!("  (resets in {rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                        }
                        lines.push(Line::from(b_spans));
                    }

                    if let Some(d7) = &u.seven_day {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("7-Day Rolling Limit: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(d7.pct, bar_width));
                        if let Some(rst) = &d7.countdown {
                            b_spans.push(Span::styled(format!("  (resets in {rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                        }
                        lines.push(Line::from(b_spans));

                        // Pace & Burn Rate Analysis
                        let pace_opt = d7.pace.as_ref().cloned().or_else(|| crate::usage::calculate_pace(d7.pct, d7.resets_at.as_deref()));
                        if let Some(pace) = &pace_opt {
                            let mut p_spans = vec![Span::raw("  ")];
                            p_spans.extend(crate::usage::render_pace_tui_spans(pace));
                            lines.push(Line::from(p_spans));
                        }
                    }

                    if let Some(sp) = &u.spend {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("Extra Spend: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(sp.pct, bar_width));
                        b_spans.push(Span::styled(
                            format!("  (${:.2} used of ${:.2} limit)", sp.used, sp.limit),
                            Style::default().fg(muted_text),
                        ));
                        lines.push(Line::from(b_spans));
                    }

                    if !u.models.is_empty() {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("Per-Model Weekly Limits:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        for m in &u.models {
                            let mut m_spans = vec![Span::styled(format!("  {:<10} ", m.name), Style::default().fg(Color::Rgb(203, 213, 225)))];
                            m_spans.extend(crate::usage::render_tui_progress_spans(m.pct, bar_width.saturating_sub(4).max(6)));
                            if let Some(rst) = &m.countdown {
                                m_spans.push(Span::styled(format!(" ({rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                            }
                            lines.push(Line::from(m_spans));
                        }
                    }

                    // Status notes
                    match u.status {
                        crate::usage::UsageStatus::Ok => {
                            if !has_limits {
                                lines.push(Line::raw(""));
                                lines.push(Line::from(vec![
                                    Span::styled("  ● Account active. Usage API reports unlimited / no active restrictions.", Style::default().fg(active_color)),
                                ]));
                            }
                        }
                        crate::usage::UsageStatus::RateLimited => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ⚠ Anthropic usage endpoint is rate-limited (HTTP 429).", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                            ]));
                            lines.push(Line::from(vec![
                                Span::styled("    Press 'u' to refresh quota when limit window resets.", Style::default().fg(muted_text)),
                            ]));
                        }
                        crate::usage::UsageStatus::TokenExpired => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ⚠ OAuth access token is expired.", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                            ]));
                            lines.push(Line::from(vec![
                                Span::styled("    Press Enter to launch Claude Code and re-authenticate in browser.", Style::default().fg(Color::White)),
                            ]));
                        }
                        crate::usage::UsageStatus::NoUsageAccess => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ℹ OAuth rate limits API not enabled on this account tier.", Style::default().fg(Color::DarkGray)),
                            ]));
                        }
                        crate::usage::UsageStatus::Unavailable => {
                            if !has_limits {
                                lines.push(Line::raw(""));
                                let msg = u.error_message.as_deref().unwrap_or("No cached usage readings yet.");
                                lines.push(Line::from(vec![
                                    Span::styled(format!("  ℹ {msg}"), Style::default().fg(Color::DarkGray)),
                                ]));
                                lines.push(Line::from(vec![
                                    Span::styled("    Press 'u' to fetch live quota from Anthropic.", Style::default().fg(muted_text)),
                                ]));
                            }
                        }
                    }
                }
                None => {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![
                        Span::styled("  No cached readings. Press 'u' to fetch live quota.", Style::default().fg(muted_text)),
                    ]));
                }
            }

            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(" QUOTA & RATE LIMITS ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)))
            )
        }
        Some((_, Item::NewProfile)) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled("ADD NEW ACCOUNT", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" (or press "),
                    Span::styled("n", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)),
                    Span::raw(") to create a new profile."),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("How it works:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  1. Choose a clean profile name (e.g. \"work\", \"personal\")."),
                ]),
                Line::from(vec![
                    Span::raw("  2. Claude Code opens and prompts browser OAuth sign-in."),
                ]),
                Line::from(vec![
                    Span::raw("  3. Credentials and rate limits stay completely isolated."),
                ]),
            ];
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(74, 222, 128)))
                    .title(Span::styled(" CREATE PROFILE ", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)))
            )
        }
        Some((_, Item::AddToken)) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled("DIRECT TOKEN / API KEY IMPORT", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" (or press "),
                    Span::styled("t", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                    Span::raw(") to register an OAuth Setup Token or API Key."),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Supported Token Formats:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  • sk-ant-oat01-... ", Style::default().fg(Color::White)),
                    Span::styled("(OAuth setup-token with quota)", Style::default().fg(muted_text)),
                ]),
                Line::from(vec![
                    Span::styled("  • sk-ant-api03-... ", Style::default().fg(Color::White)),
                    Span::styled("(Anthropic API key for pay-per-token)", Style::default().fg(muted_text)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Why use this:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  • Headless servers / remote SSH without browser access"),
                ]),
                Line::from(vec![
                    Span::raw("  • Automated CI/CD or team provisioning"),
                ]),
                Line::from(vec![
                    Span::raw("  • Direct pasting without leaving the interactive TUI"),
                ]),
            ];
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(56, 189, 248)))
                    .title(Span::styled(" REGISTER TOKEN / KEY ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)))
            )
        }
        Some((_, Item::ImportDefault)) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled("IMPORT EXISTING ACCOUNT", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" (or press "),
                    Span::styled("i", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)),
                    Span::raw(") to import existing ~/.claude login."),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("How it works:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  Copies your active ~/.claude and ~/.claude.json into an isolated"),
                ]),
                Line::from(vec![
                    Span::raw("  profile so you can switch between accounts seamlessly."),
                ]),
            ];
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(250, 204, 21)))
                    .title(Span::styled(" IMPORT ACCOUNT ", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)))
            )
        }
        None => {
            Paragraph::new(vec![Line::raw("No profiles matching search.")])
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)))
        }
    };
    f.render_widget(right_widget, main_chunks[1]);

    // 3. Bottom Status & Keybindings Area
    let status_line = match mode {
        Mode::Picking => match status {
            Some(StatusMessage::Error(e)) => Line::from(vec![
                Span::styled("✖ Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(e.clone(), Style::default().fg(Color::Red)),
            ]),
            Some(StatusMessage::Info(msg)) => Line::from(vec![
                Span::styled("✔ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(msg.clone(), Style::default().fg(Color::Green)),
            ]),
            None => Line::from(vec![
                Span::styled("Tip: ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
                Span::styled("Press ", Style::default().fg(muted_text)),
                Span::styled("[/]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" to filter, ", Style::default().fg(muted_text)),
                Span::styled("[o]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" to sort, ", Style::default().fg(muted_text)),
                Span::styled("[x]", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                Span::styled(" for parallel session, ", Style::default().fg(muted_text)),
                Span::styled("[?]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" for all shortcuts.", Style::default().fg(muted_text)),
            ]),
        },
        Mode::Search { query } => Line::from(vec![
            Span::styled("Search / Filter: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{query}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("   (Type to filter, ↑/↓ to navigate, Enter to lock, Esc to clear)", Style::default().fg(muted_text)),
        ]),
        Mode::AddToken { step, token_buf, name_buf } => match step {
            TokenStep::EnteringToken => Line::from(vec![
                Span::styled("Paste Token / Key: ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{token_buf}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("   (sk-ant-oat... or sk-ant-api..., Enter to proceed, Esc to cancel)", Style::default().fg(muted_text)),
            ]),
            TokenStep::EnteringName => Line::from(vec![
                Span::styled("Profile name: ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{name_buf}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("   (Enter to register profile, Esc to cancel)", Style::default().fg(muted_text)),
            ]),
        },
        Mode::Naming { action, buffer } => {
            let label = match action {
                Action::New => "Name for new profile",
                Action::Import => "Name for imported profile",
                Action::Rename { .. } => "New name for profile",
                Action::Alias { .. } => "Alias (blank to unset)",
            };
            match status {
                Some(StatusMessage::Error(e)) => Line::from(vec![
                    Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{buffer}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("   ({e})"), Style::default().fg(Color::Red)),
                ]),
                _ => Line::from(vec![
                    Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{buffer}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled("   (Enter to confirm, Esc to cancel)", Style::default().fg(muted_text)),
                ]),
            }
        }
        Mode::ConfirmDelete { name } => Line::from(vec![
            Span::styled(format!("Delete profile \"{name}\"? This removes its stored login. [y/N]"), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Mode::Help => Line::from(vec![
            Span::styled("Command Palette active: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("Press Esc or ? to return to picker.", Style::default().fg(Color::White)),
        ]),
    };

    let keybindings_line = Line::from(vec![
        Span::styled(" [↑/↓] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Move  "),
        Span::styled(" [Enter] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Launch  "),
        Span::styled(" [x] ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
        Span::raw("Session  "),
        Span::styled(" [s] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Switch  "),
        Span::styled(" [/] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Filter  "),
        Span::styled(" [o] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Sort  "),
        Span::styled(" [t] ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
        Span::raw("Token  "),
        Span::styled(" [u] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Refresh  "),
        Span::styled(" [?] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Help  "),
        Span::styled(" [q] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Quit"),
    ]);

    let footer = Paragraph::new(vec![status_line, keybindings_line]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" STATUS & ACTIONS ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)))
    );
    f.render_widget(footer, chunks[2]);

    // 4. Floating Command Palette / Help Modal Overlay
    if matches!(mode, Mode::Help) {
        let help_area = centered_rect(76, 76, f.area());
        f.render_widget(Clear, help_area);

        let help_text = vec![
            Line::from(vec![
                Span::styled("✦ CLAUDE-USER COMMAND PALETTE & SHORTCUTS ✦", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("NAVIGATION & SELECTION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  ↑ / ↓ / j / k       ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Navigate through accounts and actions", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  Home / End          ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Jump to top or bottom of profile list", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  PgUp / PgDn         ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Scroll profiles 5 items at a time", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("LAUNCH & EXECUTION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  Enter               ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("Launch Claude Code with selected profile (updates default)", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  x                   ", Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)),
                Span::styled("Run isolated parallel session (leaves default symlink unchanged!)", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  s                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Switch global default active profile without launching", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  u                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Fetch fresh live quota from Anthropic OAuth endpoint", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("SEARCH & SMART SORTING", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  /                   ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("Instant filter/search (matches name, alias, email, org)", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  o                   ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("Cycle sort: Most Quota Available → Soonest Reset → A-Z → Default", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("PROFILE MANAGEMENT", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  n                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Create new profile & sign in via browser OAuth", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  t                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Direct paste OAuth setup-token or API key (modal)", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  i                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Import existing ~/.claude login into managed profile", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  a                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Assign short alias (e.g. 'dev', 'work')", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  m                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Map / unmap current working directory (CWD) to profile", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  e                   ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Toggle enable / disable (held out of auto-rotation)", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  r / d               ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Rename profile  /  Delete profile login", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
            Line::from(vec![
                Span::styled("  ? / Esc             ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("Close this help dialog", Style::default().fg(Color::Rgb(203, 213, 225))),
            ]),
        ];

        let help_widget = Paragraph::new(help_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(250, 204, 21)))
                .title(Span::styled(" COMMAND PALETTE (Esc or ? to close) ", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)))
        );
        f.render_widget(help_widget, help_area);
    }
}
