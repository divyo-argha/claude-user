use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

use crate::aliases;
use crate::profiles;
use crate::usage::{self, AccountUsage, UsageStatus};

type WatchTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn run_watch(interval_secs: u64) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal: WatchTerminal = Terminal::new(backend)?;

    let res = watch_loop(&mut terminal, interval_secs.max(2));

    disable_raw_mode().ok();
    stdout().execute(LeaveAlternateScreen).ok();

    res
}

struct WatchItem {
    name: String,
    alias: Option<String>,
    is_current: bool,
    is_disabled: bool,
    usage: Option<AccountUsage>,
}

fn fetch_watch_items(force: bool) -> Result<Vec<WatchItem>> {
    let current = profiles::current_profile().unwrap_or(None);
    let all = profiles::list_profiles()?;
    let mut items = Vec::new();

    for name in all {
        let is_current = current.as_deref() == Some(&name);
        let is_disabled = profiles::is_profile_disabled(&name).unwrap_or(false);
        let my_aliases = aliases::aliases_for_profile(&name).unwrap_or_default();
        let alias = my_aliases.first().cloned();
        let usage = usage::get_profile_usage(&name, force).ok();

        items.push(WatchItem {
            name,
            alias,
            is_current,
            is_disabled,
            usage,
        });
    }

    Ok(items)
}

fn watch_loop(terminal: &mut WatchTerminal, interval_secs: u64) -> Result<()> {
    let mut items = fetch_watch_items(false)?;
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut last_fetch = Instant::now();
    let poll_interval = Duration::from_secs(interval_secs);
    let mut status_msg: Option<(String, Style)> = None;

    loop {
        terminal.draw(|f| draw_watch(f, &items, &mut list_state, interval_secs, &status_msg))?;

        // Poll keyboard events with 200ms timeout
        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = list_state.selected().unwrap_or(0);
                    if i + 1 < items.len() {
                        list_state.select(Some(i + 1));
                    }
                }
                KeyCode::Char('s') => {
                    if let Some(sel) = list_state.selected().and_then(|i| items.get(i)) {
                        match profiles::activate_profile(&sel.name) {
                            Ok(()) => {
                                status_msg = Some((
                                    format!("Switched active profile to \"{}\"", sel.name),
                                    Style::default().fg(Color::Green),
                                ));
                                items = fetch_watch_items(false)?;
                            }
                            Err(e) => {
                                status_msg = Some((
                                    format!("Switch error: {e}"),
                                    Style::default().fg(Color::Red),
                                ));
                            }
                        }
                    }
                }
                KeyCode::Char('r') => {
                    status_msg = Some((
                        "Refreshing live usage quotas...".to_string(),
                        Style::default().fg(Color::Cyan),
                    ));
                    terminal.draw(|f| draw_watch(f, &items, &mut list_state, interval_secs, &status_msg))?;
                    items = fetch_watch_items(true)?;
                    last_fetch = Instant::now();
                    status_msg = Some((
                        "Refreshed usage data from Anthropic API.".to_string(),
                        Style::default().fg(Color::Green),
                    ));
                }
                _ => {}
            }
        }

        // Automatic interval poll
        if last_fetch.elapsed() >= poll_interval {
            items = fetch_watch_items(false)?;
            last_fetch = Instant::now();
        }
    }
}

fn draw_watch(
    f: &mut Frame,
    items: &[WatchItem],
    state: &mut ListState,
    interval_secs: u64,
    status_msg: &Option<(String, Style)>,
) {
    let logo_color_1 = Color::Rgb(217, 119, 87);
    let border_color = Color::Rgb(71, 85, 105);
    let active_color = Color::Rgb(74, 222, 128);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    // Top Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" LIVE QUOTA MONITOR ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
        Span::raw(format!("(polling every {interval_secs}s)  |  ")),
        Span::styled("↑/↓", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
        Span::raw(" Select  |  "),
        Span::styled("s", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
        Span::raw(" Switch Active  |  "),
        Span::styled("r", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
        Span::raw(" Force Refresh  |  "),
        Span::styled("q", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(header, chunks[0]);

    // Profile list items
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, it)| {
            let is_selected = state.selected() == Some(idx);
            let prefix = if is_selected { " ▶ " } else { "   " };

            let name_style = if it.is_current {
                Style::default().fg(active_color).add_modifier(Modifier::BOLD)
            } else if it.is_disabled {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let alias_part = it
                .alias
                .as_ref()
                .map(|a| format!("  (@{a})"))
                .unwrap_or_default();

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(logo_color_1)),
                Span::styled(format!("{:<16}", it.name), name_style),
                Span::styled(format!("{:<10}", alias_part), Style::default().fg(Color::Yellow)),
            ];

            if it.is_current {
                spans.push(Span::styled(" ● active ", Style::default().fg(active_color).add_modifier(Modifier::BOLD)));
            } else {
                spans.push(Span::raw("          "));
            }

            match &it.usage {
                Some(u) if u.status == UsageStatus::Ok => {
                    let h5_str = u.five_hour.as_ref().map(|h| {
                        let bar = usage::render_progress_bar(h.pct, 10);
                        let rst = h.countdown.as_deref().unwrap_or("?");
                        format!("5h: {bar} ({rst})")
                    }).unwrap_or_default();

                    let d7_str = u.seven_day.as_ref().map(|d| {
                        let bar = usage::render_progress_bar(d.pct, 10);
                        let rst = d.countdown.as_deref().unwrap_or("?");
                        format!("7d: {bar} ({rst})")
                    }).unwrap_or_default();

                    spans.push(Span::styled(format!(" {h5_str}   {d7_str}"), Style::default().fg(Color::Cyan)));
                }
                Some(u) if u.status == UsageStatus::RateLimited => {
                    spans.push(Span::styled(" [Rate-Limited 429]", Style::default().fg(Color::Yellow)));
                }
                Some(u) if u.status == UsageStatus::TokenExpired => {
                    spans.push(Span::styled(" [OAuth Token Expired]", Style::default().fg(Color::Red)));
                }
                _ => {
                    spans.push(Span::styled(" [Usage Unavailable]", Style::default().fg(Color::DarkGray)));
                }
            }

            if it.is_disabled {
                spans.push(Span::styled("  (disabled)", Style::default().fg(Color::DarkGray)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" ACCOUNTS & LIMITS ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD))),
        );
    f.render_stateful_widget(list, chunks[1], state);

    // Status bar at bottom
    let (msg, style) = match status_msg {
        Some((text, st)) => (text.clone(), *st),
        None => {
            ("Running live watch. Press 's' to switch active account, 'r' to refresh, 'q' to quit.".to_string(), Style::default().fg(Color::DarkGray))
        }
    };

    let bottom = Paragraph::new(Line::from(Span::styled(msg, style)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" STATUS ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD))),
        );
    f.render_widget(bottom, chunks[2]);
}
