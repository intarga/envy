use itertools::{
    FoldWhile::{Continue, Done},
    Itertools,
};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Stdout, Write, stdin, stdout},
    ops::{Add, Sub},
};
use termion::{
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

// TODO: operate on graphemes not chars

const SCROLLOFF: usize = 5;

type Term = AlternateScreen<RawTerminal<Stdout>>;

struct CursorPosition {
    line: usize,
    char: usize,
    /// If a vertical move causes a change in cursor column (because the new line is too short),
    /// this stores the original column so it can be restored on a subsequent vertical move
    target_char: Option<usize>,
}

struct TermSize {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ViewOffset {
    line: usize,
    wrap: usize,
}

struct EditorState {
    file_text: Vec<String>,
    /// first line in file to draw
    view_offset: ViewOffset,
    cursor_position: CursorPosition,
    term_size: TermSize,
}

impl EditorState {
    fn new(path: String) -> Self {
        let (cols, rows) = termion::terminal_size().unwrap();

        let file_text = BufReader::new(File::open(path).unwrap())
            .lines()
            .collect::<Result<Vec<String>, std::io::Error>>()
            .unwrap();

        EditorState {
            file_text,
            view_offset: ViewOffset { line: 0, wrap: 0 },
            cursor_position: CursorPosition {
                line: 0,
                char: 0,
                target_char: None,
            },
            term_size: TermSize { cols, rows },
        }
    }

    fn visual_lines_in_range(&self, range: std::ops::Range<usize>) -> usize {
        range
            .map(|line_idx| {
                (self.file_text[line_idx].len().add(1) / self.term_size.cols as usize) + 1
            })
            .sum::<usize>()
    }

    fn get_cursor_visual_pos(&self) -> (u16, u16) {
        let col = ((self.cursor_position.char % self.term_size.cols as usize) + 1)
            .try_into()
            .unwrap();

        let visual_lines_above: u16 = self
            .visual_lines_in_range(self.view_offset.line..self.cursor_position.line)
            // TODO: should this be in visual_lines_in_range? or is it because terminal size is 1-based?
            .add(1)
            // account for the visual lines of the first line not shown
            .sub(self.view_offset.wrap)
            .try_into()
            .unwrap();
        let breaks_in_curr_line = self.cursor_position.char / self.term_size.cols as usize;
        let row = visual_lines_above + breaks_in_curr_line as u16;

        (col, row)
    }

    /// Number of term rows a line takes up when wrapped
    fn line_visual_height(&self, line_idx: usize) -> usize {
        self.file_text[line_idx].len() + 1 / self.term_size.cols as usize
    }

    /// Returns the view offset `distance` term rows up from the offset represented by start
    fn offset_at_visual_distance_up(&self, start: ViewOffset, distance: usize) -> ViewOffset {
        // let start_line = self.file_text.char_to_line(start);
        // let start_wrap =
        //     (self.file_text.line_to_char(start_line) - start) / self.term_size.cols as usize;

        let (logical_line, visual_distance) = (start.line..0)
            // skip current line, we account for it by using start_wrap in the fold_while acc
            .skip(1)
            .map(|line_idx| self.line_visual_height(line_idx))
            .fold_while((0, start.wrap), |(logi_acc, visu_acc), x| {
                let logi_next = logi_acc + 1;
                let visu_next = visu_acc + x;
                if visu_next > distance {
                    Done((logi_next, visu_next))
                } else {
                    Continue((logi_next, visu_next))
                }
            })
            .into_inner();

        if visual_distance < distance {
            ViewOffset { line: 0, wrap: 0 }
        } else {
            ViewOffset {
                line: logical_line,
                wrap: visual_distance - distance,
            }
        }
    }

    // TODO: better name?
    fn ensure_cursor_in_view(&mut self) {
        let offset_at_cursor = ViewOffset {
            line: self.cursor_position.line,
            wrap: (self.file_text[self.cursor_position.line].len() + 1)
                / self.term_size.cols as usize,
        };

        // lower here means visually lower on the screen, not lower number
        let lower_bound = self.offset_at_visual_distance_up(offset_at_cursor.clone(), SCROLLOFF);
        // TODO: can be calculated less redundantly relative to lower_bound
        let upper_bound = self.offset_at_visual_distance_up(
            offset_at_cursor,
            self.term_size.rows as usize - SCROLLOFF,
        );

        if self.view_offset < upper_bound {
            self.view_offset = upper_bound
        } else if self.view_offset > lower_bound {
            self.view_offset = lower_bound
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_position.char > 0 {
            self.cursor_position.char -= 1;
        } else if self.cursor_position.line > 0 {
            self.cursor_position.line -= 1;
            self.cursor_position.char = self.file_text[self.cursor_position.line].len();
        }
        self.cursor_position.target_char = None
    }

    fn cursor_right(&mut self) {
        if self.cursor_position.char < self.file_text[self.cursor_position.line].len() {
            self.cursor_position.char += 1;
        } else if self.cursor_position.line < self.file_text.len() - 1 {
            self.cursor_position.line += 1;
            self.cursor_position.char = 0;
        }
        self.cursor_position.target_char = None
    }

    fn cursor_up(&mut self) {
        if self.cursor_position.line < 1 {
            return;
        }

        let new_line = self.cursor_position.line - 1;
        let target_char = self
            .cursor_position
            .target_char
            .unwrap_or(self.cursor_position.char);
        let new_line_end = self.file_text[new_line].len();
        (self.cursor_position.char, self.cursor_position.target_char) =
            if new_line_end < target_char {
                (new_line_end, Some(target_char))
            } else {
                (target_char, None)
            };
        self.cursor_position.line = new_line;
    }

    fn cursor_down(&mut self) {
        if self.cursor_position.line >= self.file_text.len() - 1 {
            // TODO handle invalid cursor?
            return;
        }

        let new_line = self.cursor_position.line + 1;
        let target_char = self
            .cursor_position
            .target_char
            .unwrap_or(self.cursor_position.char);
        let new_line_end = self.file_text[new_line].len();
        (self.cursor_position.char, self.cursor_position.target_char) =
            if new_line_end < target_char {
                (new_line_end, Some(target_char))
            } else {
                (target_char, None)
            };
        self.cursor_position.line = new_line;
    }
}

fn init_tui() -> io::Result<Term> {
    let mut term = stdout().into_raw_mode()?;
    write!(term, "{}", termion::cursor::Save)?;
    term.flush()?;
    term.into_alternate_screen()
}

fn restore_tui() -> io::Result<()> {
    write!(
        stdout(),
        "{}{}",
        termion::screen::ToMainScreen,
        termion::cursor::Restore
    )?;
    stdout().flush()?;
    Ok(())
}

fn init_panic_hook() -> io::Result<()> {
    let raw_output = stdout().into_raw_mode()?;
    raw_output.suspend_raw_mode()?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // intentionally ignore errors here since we're already in a panic
        let _ = raw_output.suspend_raw_mode();
        let _ = restore_tui();
        original_hook(panic_info);
    }));
    Ok(())
}

fn process_keypress(key: Key, state: &mut EditorState) -> bool {
    match key {
        Key::Char('q') => return true,
        Key::Left => state.cursor_left(),
        Key::Right => state.cursor_right(),
        Key::Up => state.cursor_up(),
        Key::Down => state.cursor_down(),
        _ => (),
    }

    false
}

// TODO: more descriptive name?
fn layout(state: &EditorState) -> Vec<&str> {
    let width = state.term_size.cols as usize;
    let mut visual_lines = Vec::with_capacity(state.term_size.rows.into());
    let mut push_count = 0;

    for line in state.file_text.iter().skip(state.view_offset.line) {
        if line.len() < width {
            visual_lines.push(line.as_str());
            push_count += 1
        } else {
            // let wrap_count = ((line.len() + 1) / width) + 1;

            let mut rest = line.as_str();
            let mut char_indices = line.char_indices();
            while let Some((i, _)) = char_indices.nth(width) {
                let (p1, p2) = rest.split_at(i);
                visual_lines.push(p1);
                push_count += 1;
                rest = p2;
            }
            if !rest.is_empty() {
                visual_lines.push(rest);
                push_count += 1;
            }
        }

        if push_count - state.view_offset.wrap > state.term_size.rows.into() {
            // we have enough visual lines to fill the viewport
            break;
        }
    }

    visual_lines
}

#[test]
fn test_layout() {
    // let file_text = BufReader::new(File::open(path).unwrap())
    //     .lines()
    //     .collect::<Result<Vec<String>, std::io::Error>>()
    //     .unwrap();
    let state = EditorState {
        // file_text: Rope::from_str("\n\n1234567\n1234\n\n"),
        file_text: "\n\n1234567\n1234\n\n"
            .lines()
            .map(|l| l.to_string())
            .collect(),
        view_offset: ViewOffset { line: 2, wrap: 0 },
        cursor_position: CursorPosition {
            line: 2,
            char: 1,
            target_char: None,
        },
        term_size: TermSize { cols: 3, rows: 5 },
    };

    let visual_lines = layout(&state);

    assert_eq!(visual_lines.len(), 5);
    assert_eq!(visual_lines[0], "123");
    assert_eq!(visual_lines[1], "456");
    assert_eq!(visual_lines[2], "7\n");
    assert_eq!(visual_lines[3], "123");
    assert_eq!(visual_lines[3], "4\n");
}

fn render_editor(state: &EditorState, term: &mut Term) {
    let visual_lines = layout(state);

    write!(term, "{}", termion::clear::All).unwrap();
    for (line_num, line_text) in visual_lines.into_iter().enumerate() {
        write!(
            term,
            "{}{}",
            termion::cursor::Goto(1, (line_num + 1).try_into().unwrap()),
            line_text
        )
        .unwrap();
    }

    let (col, row) = state.get_cursor_visual_pos();
    write!(term, "{}", termion::cursor::Goto(col, row)).unwrap();

    term.flush().unwrap();
}

fn main() {
    init_panic_hook().unwrap();

    let stdin = stdin();
    let mut term = init_tui().unwrap();

    let path = std::env::args().nth(1).unwrap();

    let mut state = EditorState::new(path);

    render_editor(&state, &mut term);

    for c in stdin.keys() {
        let quit = process_keypress(c.unwrap(), &mut state);

        state.ensure_cursor_in_view();

        if quit {
            break;
        } else {
            render_editor(&state, &mut term);
        }
    }
}
