//! Miette report rendering for shader diagnostics.

use std::{
    error::Error,
    fmt::{self, Display},
};

use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, NamedSource, Severity,
    SourceCode,
};

/// Owned `miette` diagnostic for one shader diagnostic entry.
#[derive(Clone, Debug)]
pub(super) struct MietteReport {
    /// Human-readable diagnostic message.
    pub message: String,
    /// Stage/pass context rendered as help text.
    pub help: Option<String>,
    /// Source text used for span rendering.
    pub source: Option<NamedSource<String>>,
    /// Primary source label.
    pub label: Option<LabeledSpan>,
}

impl MietteReport {
    /// Renders this report as a stable ASCII `miette` diagnostic.
    pub(super) fn render(&self) -> String {
        let mut output = String::new();
        if GraphicalReportHandler::new_themed(GraphicalTheme::none())
            .with_context_lines(2)
            .render_report(&mut output, self)
            .is_err()
        {
            output = self.to_string();
        }
        output
    }
}

impl Display for MietteReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for MietteReport {}

impl Diagnostic for MietteReport {
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        Some(Box::new("shader::diagnostic"))
    }

    fn severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        self.help
            .as_ref()
            .map(Box::new)
            .map(|help| help as Box<dyn Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.source.as_ref().map(|source| source as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.label
            .as_ref()
            .map(|label| Box::new(std::iter::once(label.clone())) as Box<dyn Iterator<Item = _>>)
    }
}
