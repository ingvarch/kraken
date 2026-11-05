use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Rect, Size};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use semver::Version;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::changelog::{self, Section};
use crate::commit::Tally;
use crate::release::{Plan, push_target};
use crate::version::{Level, suggest, suggestion_warning};

const MAX_WIDTH: u16 = 64;
const MIN_WIDTH: u16 = 44;
const KEY_WIDTH: usize = 10;
/// Lines of a warning the panel shows before cutting it short.
const WARNING_LINES: usize = 2;

/// What the panel shows about the repository.
pub struct Facts {
    pub version_files: Vec<String>,
    pub current: Version,
    pub latest_tag: Option<String>,
    /// The tag the commits are counted from: the latest tag that is not a pre-release.
    pub changes_since: Option<String>,
    pub branch: String,
    /// The branch on origin to push to; `None` without an origin remote.
    pub remote_branch: Option<String>,
    pub warnings: Vec<String>,
    /// The commits since `changes_since`.
    pub commits: Tally,
}

/// A level and what releasing it would do, or why it cannot be released.
pub struct Choice {
    pub level: Level,
    pub version: Version,
    pub plan: Result<Plan, String>,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Release { plan: Box<Plan>, push: bool },
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Choose,
    Confirm,
}

pub struct App {
    facts: Facts,
    choices: Vec<Choice>,
    selected: usize,
    push: bool,
    screen: Screen,
}

impl App {
    /// Starts on the confirmation of `level` when it is given.
    pub fn new(facts: Facts, choices: Vec<Choice>, level: Option<Level>) -> App {
        let selected = level
            .or_else(|| suggest(&facts.current, &facts.commits))
            .and_then(|level| choices.iter().position(|choice| choice.level == level))
            .unwrap_or(0);
        App {
            push: facts.remote_branch.is_some(),
            facts,
            choices,
            selected,
            screen: if level.is_some() {
                Screen::Confirm
            } else {
                Screen::Choose
            },
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if is_ctrl_c(&key) || key.code == KeyCode::Char('q') {
            return Action::Quit;
        }
        match self.screen {
            Screen::Choose => match key.code {
                KeyCode::Esc => return Action::Quit,
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.choices.len() - 1)
                }
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    self.screen = Screen::Confirm
                }
                _ => {}
            },
            Screen::Confirm => match key.code {
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                    self.screen = Screen::Choose
                }
                KeyCode::Char(' ') if self.facts.remote_branch.is_some() => self.push = !self.push,
                KeyCode::Enter => {
                    if let Ok(plan) = &self.choices[self.selected].plan {
                        return Action::Release {
                            plan: Box::new(plan.clone()),
                            push: self.push && self.facts.remote_branch.is_some(),
                        };
                    }
                }
                _ => {}
            },
        }
        Action::None
    }

    /// Whether `key` may act in a terminal of `size`. When the panel does not fit,
    /// only keys that leave are let through, so nothing is released unseen.
    pub fn accepts(&self, key: KeyEvent, size: Size) -> bool {
        let leaving = is_ctrl_c(&key) || matches!(key.code, KeyCode::Esc | KeyCode::Char('q'));
        let (body, _) = self.content(text_width(size.width) as usize, body_room(size.height));
        leaving || fits(size, panel_height(&body))
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.min(MAX_WIDTH);
        let text_width = text_width(area.width);
        let (body, hints) = self.content(text_width as usize, body_room(area.height));
        let height = panel_height(&body);
        if !fits(area.as_size(), height) {
            let message = format!(
                "The terminal is too small for kraken. Make it at least {MIN_WIDTH}×{height}, or press q to quit."
            );
            frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: true }), area);
            return;
        }

        let panel = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(" kraken ").bold());
        frame.render_widget(block, panel);
        let text = |y: u16, height: u16| Rect {
            x: panel.x + 3,
            y,
            width: text_width,
            height,
        };
        frame.render_widget(
            Paragraph::new(body),
            text(panel.y + 2, height - FRAME_HEIGHT),
        );

        let separator = panel.bottom() - 3;
        let buffer = frame.buffer_mut();
        for x in panel.left()..panel.right() {
            buffer[(x, separator)].set_symbol("─");
        }
        buffer[(panel.left(), separator)].set_symbol("├");
        buffer[(panel.right() - 1, separator)].set_symbol("┤");
        frame.render_widget(Paragraph::new(hints), text(separator + 1, 1));
    }

    /// The body is at most `room` lines when it can be; only the changelog gets shorter.
    fn content(&self, width: usize, room: usize) -> (Vec<Line<'static>>, Line<'static>) {
        let mut lines = self.facts_lines(width);
        lines.push(Line::default());
        let hints = match self.screen {
            Screen::Choose => {
                self.choose_lines(&mut lines, width);
                hints(&[("↑↓", "choose"), ("enter", "continue"), ("esc", "quit")])
            }
            Screen::Confirm => self.confirm_lines(&mut lines, width, room),
        };
        (lines, hints)
    }

    fn facts_lines(&self, width: usize) -> Vec<Line<'static>> {
        let facts = &self.facts;
        let version = match (facts.version_files.as_slice(), &facts.latest_tag) {
            ([], Some(tag)) => format!("{} from tag {tag}", facts.current),
            ([], None) => format!("{}, no release tags yet", facts.current),
            (files, _) => format!("{} from {}", facts.current, files.join(", ")),
        };
        let branch = match &facts.remote_branch {
            Some(remote) => format!("{} → origin/{remote}", facts.branch),
            None => format!("{}, no origin remote", facts.branch),
        };
        let mut lines = row("Version", &version, width, Style::new());
        lines.extend(row("Branch", &branch, width, Style::new()));
        if self.screen == Screen::Choose {
            let changes = facts.commits.summary(facts.changes_since.as_deref());
            lines.extend(row("Changes", &changes, width, Style::new()));
        }
        let suggestion = (self.screen == Screen::Confirm)
            .then(|| {
                let level = self.choices[self.selected].level;
                suggestion_warning(level, suggest(&facts.current, &facts.commits))
            })
            .flatten();
        for warning in facts.warnings.iter().chain(&suggestion) {
            let mut text = wrap(&format!("! {warning}"), width);
            if text.len() > WARNING_LINES {
                text.truncate(WARNING_LINES);
                text.push("…".to_string());
            }
            lines.push(Line::default());
            lines.extend(text.into_iter().map(|line| Line::from(line).yellow()));
        }
        lines
    }

    fn choose_lines(&self, lines: &mut Vec<Line<'static>>, width: usize) {
        lines.push(Line::from("Next version").bold());
        let versions: Vec<String> = self
            .choices
            .iter()
            .map(|choice| choice.version.to_string())
            .collect();
        let column = versions.iter().map(String::len).max().unwrap_or(0);
        // Prefix, level, gap, version, gap, then the longest meaning.
        let widest = 2 + 5 + 3 + column + 3 + meaning(Level::Major).len();
        let with_meaning = widest <= width;
        for (index, (choice, version)) in self.choices.iter().zip(&versions).enumerate() {
            let selected = index == self.selected;
            let marker = if selected { "❯" } else { " " };
            let style = if selected {
                Style::new().cyan().bold()
            } else {
                Style::new()
            };
            let mut spans = vec![Span::styled(
                format!("{marker} {:<5}   {version:<column$}", choice.level),
                style,
            )];
            if with_meaning {
                spans.push(Span::styled(
                    format!("   {}", meaning(choice.level)),
                    Style::new().dim(),
                ));
            }
            lines.push(Line::from(spans));
        }
        if self.facts.current.major == 0 && self.facts.commits.breaking > 0 {
            lines.extend(
                wrap("Breaking changes bump minor in 0.x", width)
                    .into_iter()
                    .map(|line| Line::from(line).dim()),
            );
        }
    }

    fn confirm_lines(
        &self,
        lines: &mut Vec<Line<'static>>,
        width: usize,
        room: usize,
    ) -> Line<'static> {
        let choice = &self.choices[self.selected];
        let plan = match &choice.plan {
            Ok(plan) => plan,
            Err(reason) => {
                lines.push(Line::from(format!("Cannot release {}", choice.version)).bold());
                lines.extend(
                    wrap(reason, width)
                        .into_iter()
                        .map(|line| Line::from(line).red()),
                );
                return hints(&[("esc", "back")]);
            }
        };
        let title = if plan.version.pre.is_empty() {
            format!("Release {}", plan.tag)
        } else {
            format!("Release {}, a pre-release", plan.tag)
        };
        lines.push(Line::from(title).bold());
        for (spelling, files) in &plan.other_spellings {
            let verb = if files.len() == 1 { "gets" } else { "get" };
            let note = format!(
                "{} {verb} {spelling}, the PEP 440 spelling.",
                files.join(", ")
            );
            lines.extend(dim_lines(&note, width));
        }
        if let Some(version) = &plan.changelog_waits {
            let note = format!("{} gets its section with {version}.", changelog::FILE);
            lines.extend(dim_lines(&note, width));
        }
        match &plan.commit {
            Some(message) => {
                lines.extend(row("Update", &plan.files.join(", "), width, Style::new()));
                lines.extend(row("Commit", message, width, Style::new()));
                lines.extend(row("Tag", &plan.tag, width, Style::new()));
            }
            None => {
                let tag = format!("{} on the current commit", plan.tag);
                lines.extend(row("Tag", &tag, width, Style::new()));
            }
        }
        match self.push_target(plan) {
            Some(target) => {
                let check = if self.push { "[x]" } else { "[ ]" };
                let mut push = row("Push", &format!("{check} {target}"), width, Style::new());
                // The checkbox is the only control on the screen, so it takes the accent.
                if let Some(value) = push.first_mut().and_then(|line| line.spans.pop()) {
                    let rest = value.content.strip_prefix(check).unwrap_or_default();
                    let line = &mut push[0];
                    line.spans
                        .push(Span::styled(check, Style::new().cyan().bold()));
                    line.spans.push(Span::raw(rest.to_string()));
                }
                lines.extend(push);
            }
            None => lines.extend(row("Push", "no origin remote", width, Style::new().dim())),
        }
        for file in &plan.untracked {
            let note = format!("{file} is not tracked by git, so it stays out of the commit.");
            lines.extend(dim_lines(&note, width));
        }
        if let Some(section) = &plan.changelog {
            let room = room.saturating_sub(lines.len());
            lines.extend(changelog_lines(section, width, room));
        }
        if self.facts.remote_branch.is_some() {
            hints(&[
                ("enter", "release"),
                ("space", "push on/off"),
                ("esc", "back"),
            ])
        } else {
            hints(&[("enter", "release"), ("esc", "back")])
        }
    }

    fn push_target(&self, plan: &Plan) -> Option<String> {
        push_target(
            &self.facts.branch,
            self.facts.remote_branch.as_deref(),
            &plan.tag,
        )
    }
}

fn dim_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    wrap(text, width)
        .into_iter()
        .map(|line| Line::from(line).dim())
        .collect()
}

/// The changelog section in at most `room` lines, or nothing when it has no room.
fn changelog_lines(section: &Section, width: usize, room: usize) -> Vec<Line<'static>> {
    // A blank line and the heading come first.
    let Some(room) = room.checked_sub(2).filter(|&room| room > 0) else {
        return Vec::new();
    };
    let mut body = Vec::new();
    for (group, entries) in &section.groups {
        for (index, entry) in entries.iter().enumerate() {
            let key = if index == 0 { group.title() } else { "" };
            let text = match &entry.scope {
                Some(scope) => format!("{scope}: {}", entry.text),
                None => entry.text.clone(),
            };
            body.extend(row(key, &text, width, Style::new()));
        }
    }
    if body.is_empty() {
        body.push(Line::from("Only the version heading, no entries").dim());
    }
    if body.len() > room {
        let cut = body.len() - (room - 1);
        body.truncate(room - 1);
        body.push(Line::from(format!("… {cut} more lines")).dim());
    }
    let mut lines = vec![Line::default(), Line::from("Changelog").bold()];
    lines.extend(body);
    lines
}

/// The kraken that rises when a release heads for origin.
pub const KRAKEN: &str = r#"
  .-.                              .-.
 ( @ )                            ( @ )
  \ \       _.------------._       / /
   \ \    .'                `.    / /
    ) )  /   \__        __/   \  ( (
   / /  |    (@)        (@)    |  \ \
  ( (   |                      |   ) )
   \ \   \    .-~~~~~~~~-.    /   / /
    \ '-. '._  '--------`  _.` .-` /
     '-. '-._ '-.______.-` _.-` .-`
  ~~^~~'-.  '-.  \    /  .-`  .-`~~^~~
   ~~~^~~~'-._ \  \  /  / _.-`~~~^~~~
  ~~^~~~~~^~~~'~~~~~~~~~~`~~~^~~~~~^~~
"#;

/// The caption under [`KRAKEN`], centered on it.
pub fn kraken_caption() -> String {
    let caption = "RELEASE THE KRAKEN";
    let width = KRAKEN
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let indent = width.saturating_sub(caption.len()) / 2;
    format!("{}{caption}", " ".repeat(indent))
}

/// Runs the panel until the user quits or confirms a release, which it returns with the push choice.
pub fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<Option<(Plan, bool)>> {
    loop {
        terminal.draw(|frame| app.render(frame))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press || !app.accepts(key, terminal.size()?) {
            continue;
        }
        match app.handle_key(key) {
            Action::None => {}
            Action::Quit => return Ok(None),
            Action::Release { plan, push } => return Ok(Some((*plan, push))),
        }
    }
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Text width inside a panel in a terminal `terminal_width` wide: border and padding on each side.
fn text_width(terminal_width: u16) -> u16 {
    terminal_width.min(MAX_WIDTH).saturating_sub(6)
}

/// Rows around the body: border, padding, padding, separator, hints, border.
const FRAME_HEIGHT: u16 = 6;

fn panel_height(body: &[Line]) -> u16 {
    body.len() as u16 + FRAME_HEIGHT
}

/// Body lines a panel in a terminal `terminal_height` tall has room for.
fn body_room(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(FRAME_HEIGHT) as usize
}

fn fits(size: Size, panel_height: u16) -> bool {
    size.width >= MIN_WIDTH && size.height >= panel_height
}

fn meaning(level: Level) -> &'static str {
    match level {
        Level::Patch => "bug fixes",
        Level::Minor => "new features",
        Level::Major => "breaking changes",
    }
}

/// A key column and a value that wraps under itself.
fn row(key: &str, value: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    wrap(value, width.saturating_sub(KEY_WIDTH))
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let key = if index == 0 { key } else { "" };
            Line::from(vec![
                Span::styled(format!("{key:<KEY_WIDTH$}"), Style::new().dim()),
                Span::styled(part, style),
            ])
        })
        .collect()
}

fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key, action)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), Style::new().bold()));
        spans.push(Span::styled(format!(" {action}"), Style::new().dim()));
    }
    Line::from(spans)
}

/// Splits `text` into lines no wider than `width`, keeping its line breaks.
fn wrap(text: &str, width: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| wrap_line(line, width))
        .collect()
}

/// Breaks one line at spaces where it can, measuring terminal columns.
fn wrap_line(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let used = line.width();
        if used > 0 && used + 1 + word.width() <= width {
            line.push(' ');
            line.push_str(word);
            continue;
        }
        if used > 0 {
            lines.push(std::mem::take(&mut line));
        }
        for c in word.chars() {
            if line.width() + c.width().unwrap_or(0) > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::layout::Size;
    use ratatui::style::Color;

    use crate::changelog::{Entry, Group};

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn facts() -> Facts {
        Facts {
            version_files: vec!["package.json".to_string()],
            current: v("1.2.3"),
            latest_tag: Some("v1.2.3".to_string()),
            changes_since: Some("v1.2.3".to_string()),
            branch: "main".to_string(),
            remote_branch: Some("main".to_string()),
            warnings: vec![],
            commits: Tally::default(),
        }
    }

    fn tally(breaking: usize, features: usize, fixes: usize, other: usize) -> Tally {
        Tally {
            breaking,
            features,
            fixes,
            other,
        }
    }

    fn facts_with(commits: Tally) -> Facts {
        Facts { commits, ..facts() }
    }

    fn plan(version: &str) -> Plan {
        let tag = format!("v{version}");
        Plan {
            version: v(version),
            commit: Some(format!("chore(release): {tag}")),
            tag,
            files: vec!["package.json".to_string(), "package-lock.json".to_string()],
            untracked: vec![],
            changelog: None,
            links: None,
            other_spellings: vec![],
            changelog_waits: None,
        }
    }

    fn choice(level: Level, version: &str) -> Choice {
        Choice {
            level,
            version: v(version),
            plan: Ok(plan(version)),
        }
    }

    fn choices() -> Vec<Choice> {
        vec![
            choice(Level::Patch, "1.2.4"),
            choice(Level::Minor, "1.3.0"),
            choice(Level::Major, "2.0.0"),
        ]
    }

    fn pre_choices(current: &str, label: &str) -> Vec<Choice> {
        [
            (Level::Patch, bump_text(current, Level::Patch)),
            (Level::Minor, bump_text(current, Level::Minor)),
            (Level::Major, bump_text(current, Level::Major)),
        ]
        .into_iter()
        .map(|(level, base)| choice(level, &format!("{base}-{label}.1")))
        .collect()
    }

    fn bump_text(current: &str, level: Level) -> String {
        crate::version::next(&v(current), level, None)
            .unwrap()
            .to_string()
    }

    fn app() -> App {
        App::new(facts(), choices(), None)
    }

    fn press(app: &mut App, code: KeyCode) -> Action {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn screen_sized(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = buffer
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        rows.join("\n")
    }

    fn screen(app: &App) -> String {
        screen_sized(app, 80, 24)
    }

    fn assert_shows(app: &App, text: &str) {
        let screen = screen(app);
        assert!(screen.contains(text), "missing {text:?} in\n{screen}");
    }

    fn assert_hides(app: &App, text: &str) {
        let screen = screen(app);
        assert!(!screen.contains(text), "unexpected {text:?} in\n{screen}");
    }

    #[test]
    fn shows_version_branch_and_levels() {
        let app = app();
        assert_shows(&app, "Version   1.2.3 from package.json");
        assert_shows(&app, "Branch    main → origin/main");
        assert_shows(&app, "❯ patch   1.2.4   bug fixes");
        assert_shows(&app, "  minor   1.3.0   new features");
        assert_shows(&app, "  major   2.0.0   breaking changes");
    }

    #[test]
    fn shows_tag_source_without_version_files() {
        let mut facts = facts();
        facts.version_files.clear();
        let app = App::new(facts, choices(), None);
        assert_shows(&app, "Version   1.2.3 from tag v1.2.3");
    }

    #[test]
    fn shows_branch_without_origin() {
        let mut facts = facts();
        facts.remote_branch = None;
        let app = App::new(facts, choices(), None);
        assert_shows(&app, "Branch    main, no origin remote");
    }

    #[test]
    fn shows_the_warnings() {
        let mut facts = facts();
        facts.warnings = vec![
            "latest tag v1.0.0 differs from version 1.2.3".to_string(),
            "could not compare tags with origin".to_string(),
        ];
        let app = App::new(facts, choices(), None);
        assert_shows(&app, "latest tag v1.0.0 differs from version 1.2.3");
        assert_shows(&app, "could not compare tags with origin");
    }

    #[test]
    fn arrows_and_jk_move_the_selection_within_the_list() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        assert_shows(&app, "❯ minor");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Down);
        assert_shows(&app, "❯ major");
        press(&mut app, KeyCode::Up);
        assert_shows(&app, "❯ minor");
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Up);
        assert_shows(&app, "❯ patch");
    }

    #[test]
    fn esc_q_and_ctrl_c_quit_from_the_list() {
        assert_eq!(press(&mut app(), KeyCode::Esc), Action::Quit);
        assert_eq!(press(&mut app(), KeyCode::Char('q')), Action::Quit);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app().handle_key(ctrl_c), Action::Quit);
    }

    #[test]
    fn enter_shows_what_the_release_does() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        assert_eq!(press(&mut app, KeyCode::Enter), Action::None);
        assert_shows(&app, "Release v1.3.0");
        assert_shows(&app, "Update    package.json, package-lock.json");
        assert_shows(&app, "Commit    chore(release): v1.3.0");
        assert_shows(&app, "Tag       v1.3.0");
        assert_shows(&app, "Push      [x] main and v1.3.0 to origin");
        assert_shows(&app, "enter release");
    }

    #[test]
    fn esc_goes_back_from_the_confirmation() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert_shows(&app, "❯ patch");
    }

    fn color_of(app: &App, text: &str) -> Color {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let count = text.chars().count();
        for row in terminal.backend().buffer().content().chunks(80) {
            for start in 0..=row.len() - count {
                let found: String = row[start..start + count]
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                if found == text {
                    return row[start].fg;
                }
            }
        }
        panic!("{text:?} is not on the screen");
    }

    #[test]
    fn the_push_checkbox_stands_out() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);
        assert_eq!(color_of(&app, "[x]"), Color::Cyan);
        assert_eq!(color_of(&app, "main and v1.2.4"), Color::Reset);
    }

    #[test]
    fn space_turns_the_push_off_and_on() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));
        assert_shows(&app, "Push      [ ] main and v1.2.4 to origin");
        press(&mut app, KeyCode::Char(' '));
        assert_shows(&app, "Push      [x] main and v1.2.4 to origin");
    }

    #[test]
    fn names_the_upstream_branch() {
        let mut facts = facts();
        facts.remote_branch = Some("release".to_string());
        let mut app = App::new(facts, choices(), None);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "[x] main to origin/release and v1.2.4");
    }

    #[test]
    fn enter_on_the_confirmation_releases() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));

        let action = press(&mut app, KeyCode::Enter);

        assert_eq!(
            action,
            Action::Release {
                plan: Box::new(plan("1.2.4")),
                push: false
            }
        );
    }

    #[test]
    fn starts_on_the_confirmation_when_the_level_is_given() {
        let app = App::new(facts(), choices(), Some(Level::Minor));
        assert_shows(&app, "Release v1.3.0");
    }

    #[test]
    fn without_origin_there_is_nothing_to_push() {
        let mut facts = facts();
        facts.remote_branch = None;
        let mut app = App::new(facts, choices(), None);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "Push      no origin remote");
        assert_hides(&app, "space");

        let action = press(&mut app, KeyCode::Enter);

        assert_eq!(
            action,
            Action::Release {
                plan: Box::new(plan("1.2.4")),
                push: false
            }
        );
    }

    #[test]
    fn mentions_untracked_files() {
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["Cargo.lock".to_string()];
        }
        let mut app = App::new(facts(), choices, None);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "Cargo.lock is not tracked by git");
    }

    #[test]
    fn tag_only_release_has_no_commit() {
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.files.clear();
            plan.commit = None;
        }
        let mut app = App::new(facts(), choices, None);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "Tag       v1.2.4 on the current commit");
        assert_hides(&app, "Commit");
    }

    #[test]
    fn blocked_level_shows_why_and_cannot_be_released() {
        let mut choices = choices();
        choices[0].plan = Err("tag v1.2.4 already exists".to_string());
        let mut app = App::new(facts(), choices, None);
        press(&mut app, KeyCode::Enter);

        assert_eq!(press(&mut app, KeyCode::Enter), Action::None);

        assert_shows(&app, "tag v1.2.4 already exists");
        assert_hides(&app, "enter release");
    }

    #[test]
    fn small_terminal_asks_for_more_room() {
        let screen = screen_sized(&app(), 30, 8);
        assert!(screen.contains("too small"), "{screen}");
    }

    #[test]
    fn small_terminal_lets_only_leaving_keys_through() {
        let app = app();
        let small = Size::new(30, 8);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        assert!(!app.accepts(key(KeyCode::Enter), small));
        assert!(!app.accepts(key(KeyCode::Char(' ')), small));
        assert!(app.accepts(key(KeyCode::Esc), small));
        assert!(app.accepts(key(KeyCode::Char('q')), small));
        assert!(app.accepts(key(KeyCode::Enter), Size::new(80, 24)));
    }

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("aa bb cc dd", 5), ["aa bb", "cc dd"]);
        assert_eq!(wrap("abcdefgh", 3), ["abc", "def", "gh"]);
        assert_eq!(wrap("", 5), [""]);
    }

    #[test]
    fn wrap_measures_terminal_columns() {
        assert_eq!(wrap("表示確認", 4), ["表示", "確認"]);
        assert_eq!(wrap("確認 ok", 6), ["確認", "ok"]);
    }

    #[test]
    fn small_terminal_says_how_to_leave() {
        let screen = screen_sized(&app(), 30, 8);
        assert!(screen.contains("press q to quit"), "{screen}");
    }

    #[test]
    fn confirmation_with_long_warnings_fits_a_standard_terminal() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection(), git_rejection()];
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["Cargo.lock".to_string()];
        }
        let mut app = App::new(facts, choices, None);
        press(&mut app, KeyCode::Enter);

        assert_hides(&app, "too small");
        assert_shows(&app, "enter release");
    }

    #[test]
    fn confirmation_shows_the_warning_about_no_commits() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection(), "no commits since v1.2.3".to_string()];
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["Cargo.lock".to_string()];
        }
        let mut app = App::new(facts, choices, None);
        press(&mut app, KeyCode::Enter);

        assert_shows(&app, "! no commits since v1.2.3");
        assert_hides(&app, "too small");
        assert_shows(&app, "enter release");
    }

    #[test]
    fn kraken_fits_the_narrowest_panel() {
        let art: Vec<&str> = KRAKEN.trim_matches('\n').lines().collect();
        assert!(art.len() > 5);
        assert!(art.iter().all(|line| line.width() <= MIN_WIDTH as usize));
    }

    #[test]
    fn kraken_caption_is_centered_under_the_art() {
        let width = KRAKEN.lines().map(UnicodeWidthStr::width).max().unwrap();
        let caption = kraken_caption();
        let left = caption.len() - caption.trim_start().len();

        assert_eq!(caption.trim(), "RELEASE THE KRAKEN");
        assert!(
            left.abs_diff(width - left - caption.trim().len()) <= 1,
            "{caption:?}"
        );
    }

    #[test]
    fn wrap_keeps_line_breaks() {
        assert_eq!(wrap("one\ntwo three", 20), ["one", "two three"]);
    }

    fn git_rejection() -> String {
        let hints = (1..=12)
            .map(|n| format!("hint: line {n} of advice from git about the rejected push"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("push failed: ! [rejected] main -> main (fetch first)\n{hints}")
    }

    fn entry(scope: Option<&str>, text: &str) -> Entry {
        Entry {
            scope: scope.map(str::to_string),
            text: text.to_string(),
        }
    }

    fn with_changelog(groups: Vec<(Group, Vec<Entry>)>) -> Vec<Choice> {
        let mut choices = choices();
        for choice in &mut choices {
            if let Ok(plan) = &mut choice.plan {
                plan.changelog = Some(Section {
                    version: plan.version.clone(),
                    date: "2026-11-04".to_string(),
                    groups: groups.clone(),
                });
            }
        }
        choices
    }

    fn long_changelog() -> Vec<Choice> {
        let entries = (1..=40)
            .map(|n| entry(None, &format!("change {n}")))
            .collect();
        with_changelog(vec![(Group::Changed, entries)])
    }

    #[test]
    fn confirmation_shows_the_changelog_section() {
        let choices = with_changelog(vec![
            (
                Group::Added,
                vec![
                    entry(Some("ui"), "add the release panel"),
                    entry(None, "read gradle.properties"),
                ],
            ),
            (
                Group::Fixed,
                vec![entry(None, "keep comments in deno.jsonc")],
            ),
        ]);
        let mut app = App::new(facts(), choices, None);
        press(&mut app, KeyCode::Enter);

        assert_shows(&app, "Changelog");
        assert_shows(&app, "Added     ui: add the release panel");
        assert_shows(&app, "          read gradle.properties");
        assert_shows(&app, "Fixed     keep comments in deno.jsonc");
    }

    #[test]
    fn says_when_the_changelog_section_has_no_entries() {
        let mut app = App::new(facts(), with_changelog(vec![]), None);
        press(&mut app, KeyCode::Enter);

        assert_shows(&app, "Only the version heading, no entries");
    }

    #[test]
    fn no_changelog_section_without_a_changelog() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);

        assert_hides(&app, "Changelog");
    }

    #[test]
    fn long_changelog_is_cut_to_fit_the_terminal() {
        let mut app = App::new(facts(), long_changelog(), None);
        press(&mut app, KeyCode::Enter);

        assert_hides(&app, "too small");
        assert_shows(&app, "enter release");
        assert_shows(&app, "Changed   change 1");
        assert_shows(&app, "… 33 more lines");
        assert_hides(&app, "change 40");
    }

    #[test]
    fn changelog_needs_room_for_its_heading_and_a_line() {
        let section = Section {
            version: v("1.3.0"),
            date: "2026-11-04".to_string(),
            groups: vec![(Group::Added, vec![entry(None, "a"), entry(None, "b")])],
        };
        let text = |room| -> Vec<String> {
            changelog_lines(&section, 50, room)
                .iter()
                .map(Line::to_string)
                .collect()
        };

        assert!(text(2).is_empty());
        assert_eq!(text(3), ["", "Changelog", "… 2 more lines"]);
        assert_eq!(text(4), ["", "Changelog", "Added     a", "          b"]);
    }

    #[test]
    fn changelog_gives_way_to_warnings() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection(), git_rejection()];
        let mut app = App::new(facts, long_changelog(), None);
        press(&mut app, KeyCode::Enter);

        assert_hides(&app, "too small");
        assert_shows(&app, "enter release");
    }

    #[test]
    fn long_warning_is_shortened_to_fit() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection()];
        let app = App::new(facts, choices(), None);

        assert_shows(&app, "! push failed");
        assert_hides(&app, "line 12");
        assert_hides(&app, "too small");
    }

    #[test]
    fn shows_the_changes_since_the_latest_tag() {
        let app = App::new(facts_with(tally(0, 2, 1, 0)), choices(), None);
        assert_shows(&app, "Changes   2 features, 1 fix since v1.2.3");
    }

    #[test]
    fn says_none_without_new_commits() {
        let app = app();
        assert_shows(&app, "Changes   none since v1.2.3");
        assert_shows(&app, "❯ patch");
    }

    #[test]
    fn preselects_the_suggested_level() {
        let features = App::new(facts_with(tally(0, 2, 1, 0)), choices(), None);
        assert_shows(&features, "❯ minor");
        let breaking = App::new(facts_with(tally(1, 0, 0, 0)), choices(), None);
        assert_shows(&breaking, "❯ major");
    }

    #[test]
    fn changes_row_stays_off_the_confirmation() {
        let mut app = App::new(facts_with(tally(0, 1, 0, 0)), choices(), None);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "Release v1.3.0");
        assert_hides(&app, "Changes");
    }

    #[test]
    fn notes_the_0x_rule() {
        let mut facts = facts_with(tally(1, 2, 0, 0));
        facts.current = v("0.3.0");
        let app = App::new(facts, choices(), None);
        assert_shows(&app, "Changes   1 breaking change, 2 features since v1.2.3");
        assert_shows(&app, "❯ minor");
        assert_shows(&app, "Breaking changes bump minor in 0.x");
    }

    #[test]
    fn no_0x_note_without_a_breaking_change() {
        let mut facts = facts_with(tally(0, 2, 0, 0));
        facts.current = v("0.3.0");
        assert_hides(&App::new(facts, choices(), None), "bump minor in 0.x");
        let breaking = App::new(facts_with(tally(1, 0, 0, 0)), choices(), None);
        assert_hides(&breaking, "bump minor in 0.x");
    }

    #[test]
    fn warns_when_the_given_level_is_below_the_suggestion() {
        let app = App::new(facts_with(tally(0, 1, 0, 0)), choices(), Some(Level::Patch));
        assert_shows(&app, "! the new commits suggest a minor release");
    }

    #[test]
    fn warns_after_moving_below_the_suggestion() {
        let mut app = App::new(facts_with(tally(0, 1, 0, 0)), choices(), None);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Enter);
        assert_shows(&app, "! the new commits suggest a minor release");
    }

    #[test]
    fn no_warning_at_or_above_the_suggestion() {
        for level in [Level::Minor, Level::Major] {
            let app = App::new(facts_with(tally(0, 1, 0, 0)), choices(), Some(level));
            assert_hides(&app, "suggest");
        }
    }

    #[test]
    fn no_warning_on_the_level_screen() {
        let mut app = App::new(facts_with(tally(0, 1, 0, 0)), choices(), None);
        press(&mut app, KeyCode::Up);
        assert_hides(&app, "suggest");
    }

    #[test]
    fn level_screen_fits_44_by_14() {
        let mut facts = facts_with(tally(0, 1, 0, 0));
        facts.remote_branch = None;
        let app = App::new(facts, choices(), None);
        let screen = screen_sized(&app, 44, 14);
        assert!(!screen.contains("too small"), "{screen}");
        assert!(
            screen.contains("Changes   1 feature since v1.2.3"),
            "{screen}"
        );
    }

    #[test]
    fn suggestion_warning_with_a_long_warning_fits_a_standard_terminal() {
        let mut facts = facts_with(tally(0, 1, 0, 0));
        facts.warnings = vec![git_rejection()];
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["Cargo.lock".to_string()];
        }
        let app = App::new(facts, choices, Some(Level::Patch));

        assert_hides(&app, "too small");
        assert_shows(&app, "! the new commits suggest a minor release");
        assert_shows(&app, "enter release");
    }

    #[test]
    fn rows_show_the_choice_version() {
        let app = App::new(facts(), pre_choices("1.2.3", "rc"), None);

        assert_shows(&app, "❯ patch   1.2.4-rc.1   bug fixes");
        assert_shows(&app, "  minor   1.3.0-rc.1   new features");
        assert_shows(&app, "  major   2.0.0-rc.1   breaking changes");
    }

    #[test]
    fn pre_release_rows_fit_44_by_14() {
        let mut facts = facts();
        facts.remote_branch = None;
        let app = App::new(facts, pre_choices("1.2.3", "alpha"), None);

        let screen = screen_sized(&app, 44, 14);

        assert!(!screen.contains("too small"), "{screen}");
        assert!(screen.contains("major   2.0.0-alpha.1"), "{screen}");
        assert!(screen.contains("patch   1.2.4-alpha.1"), "{screen}");
        for meaning in ["breaking changes", "new features", "bug fixes", "brea"] {
            assert!(!screen.contains(meaning), "{meaning}\n{screen}");
        }
    }

    #[test]
    fn rows_keep_the_meaning_when_they_fit() {
        let screen = screen_sized(&app(), 44, 14);
        assert!(
            screen.contains("major   2.0.0   breaking changes"),
            "{screen}"
        );
    }

    fn pre_release_app() -> App {
        let mut choices = vec![choice(Level::Minor, "1.4.0-rc.1")];
        if let Ok(plan) = &mut choices[0].plan {
            plan.changelog_waits = Some(v("1.4.0"));
            plan.other_spellings = vec![(
                "1.4.0rc1".to_string(),
                vec!["pyproject.toml".to_string(), "uv.lock".to_string()],
            )];
        }
        App::new(facts(), choices, Some(Level::Minor))
    }

    #[test]
    fn confirmation_names_a_pre_release() {
        let app = pre_release_app();
        assert_shows(&app, "Release v1.4.0-rc.1, a pre-release");
        assert_hides(&app, "Release v1.4.0-rc.1\n");
    }

    #[test]
    fn confirmation_names_another_spelling() {
        let app = pre_release_app();
        assert_shows(&app, "pyproject.toml, uv.lock get 1.4.0rc1, the PEP 440");
        assert_shows(&app, "spelling.");
    }

    #[test]
    fn confirmation_says_the_changelog_waits() {
        let app = pre_release_app();
        assert_shows(&app, "CHANGELOG.md gets its section with 1.4.0.");
    }

    #[test]
    fn one_file_in_another_spelling_gets_it() {
        let mut choices = vec![choice(Level::Minor, "1.4.0-rc.1")];
        if let Ok(plan) = &mut choices[0].plan {
            plan.other_spellings =
                vec![("1.4.0rc1".to_string(), vec!["pyproject.toml".to_string()])];
        }
        let app = App::new(facts(), choices, Some(Level::Minor));
        assert_shows(&app, "pyproject.toml gets 1.4.0rc1, the PEP 440");
    }

    #[test]
    fn a_final_release_has_no_pre_release_lines() {
        let app = App::new(facts(), choices(), Some(Level::Minor));
        assert_shows(&app, "Release v1.3.0");
        assert_hides(&app, "a pre-release");
        assert_hides(&app, "spelling");
    }

    #[test]
    fn pre_release_confirmation_fits_a_standard_terminal() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection()];
        let mut choices = vec![choice(Level::Minor, "1.4.0-rc.1")];
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["uv.lock".to_string()];
            plan.changelog_waits = Some(v("1.4.0"));
            plan.other_spellings = vec![(
                "1.4.0rc1".to_string(),
                vec!["pyproject.toml".to_string(), "uv.lock".to_string()],
            )];
        }
        let app = App::new(facts, choices, Some(Level::Minor));

        assert_hides(&app, "too small");
        assert_shows(&app, "enter release");
        assert_shows(&app, "CHANGELOG.md gets its section");
    }

    #[test]
    fn cannot_release_names_the_choice_version() {
        let mut choices = vec![choice(Level::Minor, "1.4.0-rc.1")];
        choices[0].plan = Err("project.yml: MARKETING_VERSION takes numbers only".to_string());
        let app = App::new(facts(), choices, Some(Level::Minor));

        assert_shows(&app, "Cannot release 1.4.0-rc.1");
    }

    #[test]
    fn changes_row_names_the_latest_final_tag() {
        let mut facts = facts_with(tally(0, 1, 0, 0));
        facts.current = v("1.3.0-rc.1");
        facts.latest_tag = Some("v1.3.0-rc.1".to_string());
        let app = App::new(facts, choices(), None);

        assert_shows(&app, "Changes   1 feature since v1.2.3");
        assert_shows(&app, "1.3.0-rc.1 from package.json");
    }
}
