use crate::Writer;
use core::fmt;
use core::fmt::Write;
use log;
use once_cell::unsync::OnceCell;
use crate::mutex::Mutex;

pub struct DecoratedLog<'writer, 'a, W: fmt::Write> {
    writer: &'writer mut W,
    level: log::Level,
    at_line_start: bool,
    file: &'a str,
    line: u32,
}

impl<'writer, 'a, W: fmt::Write> fmt::Write for DecoratedLog<'writer, 'a, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut lines = s.lines();

        let first = lines.next().unwrap_or("");
        if self.at_line_start {
            write!(
                self.writer,
                "[{:>5}]: {:>12}@{:03}: ",
                self.level, self.file, self.line
            )?;
            self.at_line_start = false;
        }
        write!(self.writer, "{}", first)?;

        for line in lines {
            write!(self.writer, "\n{}: {}", self.level, line)?;
        }

        if let Some('\n') = s.chars().last() {
            writeln!(self.writer)?;
            self.at_line_start = true;
        }

        Ok(())
    }
}

impl<'writer, 'a, W: fmt::Write> DecoratedLog<'writer, 'a, W> {
    pub fn write(
        writer: &'writer mut W,
        level: log::Level,
        args: &fmt::Arguments,
        file: &'a str,
        line: u32,
    ) -> fmt::Result {
        let mut decorated_writer = DecoratedLog {
            writer,
            level,
            at_line_start: true,
            file,
            line,
        };
        writeln!(decorated_writer, "{}", *args)
    }
}

impl<'writer, 'a, 'b, const N_ROW: usize, const N_COLUMN: usize>
    DecoratedLog<'writer, 'a, Writer<'b, N_ROW, N_COLUMN>>
{
    pub fn flush(&mut self) {
        self.writer.flush();
    }
}

pub struct CharWriter<const N_CHAR_PER_LINE: usize, const N_WRITEABLE_LINE: usize>(
    pub Mutex<OnceCell<Writer<'static, N_WRITEABLE_LINE, N_CHAR_PER_LINE>>>,
);

impl<const N_CHAR_PER_LINE: usize, const N_WRITEABLE_LINE: usize>
    CharWriter<N_CHAR_PER_LINE, N_WRITEABLE_LINE>
{
    pub fn lock(
        &self,
    ) -> spin::MutexGuard<'_, OnceCell<Writer<'static, N_WRITEABLE_LINE, N_CHAR_PER_LINE>>> {
        crate::lock!(self.0)
    }
}

impl<const N_CHAR_PER_LINE: usize, const N_WRITEABLE_LINE: usize> log::Log
    for CharWriter<N_CHAR_PER_LINE, N_WRITEABLE_LINE>
{
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        crate::lock!(self.0).get().is_some()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let mut guard = crate::lock!(self.0);
            let writer = guard.get_mut().unwrap();
            DecoratedLog::write(
                writer,
                record.level(),
                record.args(),
                record.file().unwrap_or("<unknown>"),
                record.line().unwrap_or(0),
            )
            .unwrap();
        }
    }

    fn flush(&self) {
        let mut guard = crate::lock!(self.0);
        guard.get_mut().unwrap().flush();
    }
}
