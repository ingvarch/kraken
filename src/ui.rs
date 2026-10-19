use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Rect, Size};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use semver::Version;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::release::{Plan, push_target};
use crate::version::{Level, bump};

const MAX_WIDTH: u16 = 64;
const MIN_WIDTH: u16 = 44;
const KEY_WIDTH: usize = 10;
/// Lines of a warning the panel shows before cutting it short.
const WARNING_LINES: usize = 2;

/// What the panel shows about the repository.
pub struct Facts {
    pub version_files: Vec<&'static str>,
    pub current: Version,
    pub latest_tag: Option<String>,
    pub branch: String,
    /// The branch on origin to push to; `None` without an origin remote.
    pub remote_branch: Option<String>,
    pub warnings: Vec<String>,
}

/// A level and what releasing it would do, or why it cannot be released.
pub struct Choice {
    pub level: Level,
    pub plan: Result<Plan, String>,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Release { plan: Plan, push: bool },
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
                            plan: plan.clone(),
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
        let (body, _) = self.content(text_width(size.width) as usize);
        leaving || fits(size, panel_height(&body))
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.min(MAX_WIDTH);
        let text_width = text_width(area.width);
        let (body, hints) = self.content(text_width as usize);
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
        frame.render_widget(Paragraph::new(body), text(panel.y + 2, height - 6));

        let separator = panel.bottom() - 3;
        let buffer = frame.buffer_mut();
        for x in panel.left()..panel.right() {
            buffer[(x, separator)].set_symbol("─");
        }
        buffer[(panel.left(), separator)].set_symbol("├");
        buffer[(panel.right() - 1, separator)].set_symbol("┤");
        frame.render_widget(Paragraph::new(hints), text(separator + 1, 1));
    }

    fn content(&self, width: usize) -> (Vec<Line<'static>>, Line<'static>) {
        let mut lines = self.facts_lines(width);
        lines.push(Line::default());
        let hints = match self.screen {
            Screen::Choose => {
                self.choose_lines(&mut lines);
                hints(&[("↑↓", "choose"), ("enter", "continue"), ("esc", "quit")])
            }
            Screen::Confirm => self.confirm_lines(&mut lines, width),
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
        for warning in &facts.warnings {
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

    fn choose_lines(&self, lines: &mut Vec<Line<'static>>) {
        lines.push(Line::from("Next version").bold());
        let versions: Vec<String> = self
            .choices
            .iter()
            .map(|choice| bump(&self.facts.current, choice.level).to_string())
            .collect();
        let column = versions.iter().map(String::len).max().unwrap_or(0);
        for (index, (choice, version)) in self.choices.iter().zip(&versions).enumerate() {
            let selected = index == self.selected;
            let marker = if selected { "❯" } else { " " };
            let style = if selected {
                Style::new().cyan().bold()
            } else {
                Style::new()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{marker} {:<5}   {version:<column$}", choice.level),
                    style,
                ),
                Span::styled(format!("   {}", meaning(choice.level)), Style::new().dim()),
            ]));
        }
    }

    fn confirm_lines(&self, lines: &mut Vec<Line<'static>>, width: usize) -> Line<'static> {
        let choice = &self.choices[self.selected];
        let plan = match &choice.plan {
            Ok(plan) => plan,
            Err(reason) => {
                let version = bump(&self.facts.current, choice.level);
                lines.push(Line::from(format!("Cannot release {version}")).bold());
                lines.extend(
                    wrap(reason, width)
                        .into_iter()
                        .map(|line| Line::from(line).red()),
                );
                return hints(&[("esc", "back")]);
            }
        };
        lines.push(Line::from(format!("Release {}", plan.tag)).bold());
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
            lines.extend(
                wrap(&note, width)
                    .into_iter()
                    .map(|line| Line::from(line).dim()),
            );
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
            Action::Release { plan, push } => return Ok(Some((plan, push))),
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

/// Border, padding, body, padding, separator, hints, border.
fn panel_height(body: &[Line]) -> u16 {
    body.len() as u16 + 6
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

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn facts() -> Facts {
        Facts {
            version_files: vec!["package.json"],
            current: v("1.2.3"),
            latest_tag: Some("v1.2.3".to_string()),
            branch: "main".to_string(),
            remote_branch: Some("main".to_string()),
            warnings: vec![],
        }
    }

    fn plan(version: &str) -> Plan {
        let tag = format!("v{version}");
        Plan {
            version: v(version),
            commit: Some(format!("chore(release): {tag}")),
            tag,
            files: vec!["package.json", "package-lock.json"],
            untracked: vec![],
        }
    }

    fn choices() -> Vec<Choice> {
        vec![
            Choice {
                level: Level::Patch,
                plan: Ok(plan("1.2.4")),
            },
            Choice {
                level: Level::Minor,
                plan: Ok(plan("1.3.0")),
            },
            Choice {
                level: Level::Major,
                plan: Ok(plan("2.0.0")),
            },
        ]
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
                plan: plan("1.2.4"),
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
                plan: plan("1.2.4"),
                push: false
            }
        );
    }

    #[test]
    fn mentions_untracked_files() {
        let mut choices = choices();
        if let Ok(plan) = &mut choices[0].plan {
            plan.untracked = vec!["Cargo.lock"];
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
            plan.untracked = vec!["Cargo.lock"];
        }
        let mut app = App::new(facts, choices, None);
        press(&mut app, KeyCode::Enter);

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

    #[test]
    fn long_warning_is_shortened_to_fit() {
        let mut facts = facts();
        facts.warnings = vec![git_rejection()];
        let app = App::new(facts, choices(), None);

        assert_shows(&app, "! push failed");
        assert_hides(&app, "line 12");
        assert_hides(&app, "too small");
    }
}
