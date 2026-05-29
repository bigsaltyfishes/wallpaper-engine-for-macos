//! Miette report rendering for shader diagnostics.

use std::{
    error::Error,
    fmt::{self, Display},
};

use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, NamedSource, Severity,
    SourceCode,
};

use super::ShaderDiagnostic;

/// Borrowed source context used to create a miette report.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReportContext<'a> {
    /// Diagnostic data to render.
    pub(super) diagnostic: &'a ShaderDiagnostic,
    /// Generated source text used for span rendering.
    pub(super) source: Option<&'a str>,
}

/// Owned `miette` diagnostic for one shader diagnostic entry.
#[derive(Clone, Debug)]
pub(super) struct MietteReport {
    /// Human-readable diagnostic message.
    message: String,
    /// Stage/pass context rendered as help text.
    help: Option<String>,
    /// Source text used for span rendering.
    source: Option<NamedSource<String>>,
    /// Primary source label.
    label: Option<LabeledSpan>,
}

impl From<ReportContext<'_>> for MietteReport {
    fn from(context: ReportContext<'_>) -> Self {
        let diagnostic = context.diagnostic;
        let source = context.source.map(|source| {
            NamedSource::new(
                diagnostic
                    .generated_source_path()
                    .unwrap_or("generated/shader.glsl"),
                source.to_owned(),
            )
            .with_language("glsl")
        });
        let label = diagnostic.span().map(|span| {
            LabeledSpan::new_primary_with_span(
                diagnostic.pass().map(ToOwned::to_owned),
                (span.start(), span.end().saturating_sub(span.start())),
            )
        });

        Self {
            message: diagnostic.message().to_owned(),
            help: DiagnosticHelp::from(diagnostic).into_option(),
            source,
            label,
        }
    }
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

/// Stage and pass context formatted for report help text.
#[derive(Debug)]
struct DiagnosticHelp(String);

impl DiagnosticHelp {
    /// Returns the help string when it contains context.
    fn into_option(self) -> Option<String> {
        (!self.0.is_empty()).then_some(self.0)
    }
}

impl From<&ShaderDiagnostic> for DiagnosticHelp {
    fn from(diagnostic: &ShaderDiagnostic) -> Self {
        let mut parts = Vec::new();
        if let Some(stage) = diagnostic.stage() {
            parts.push(format!("stage: {stage:?}"));
        }
        if let Some(pass) = diagnostic.pass() {
            parts.push(format!("pass: {pass}"));
        }
        if let Some(path) = diagnostic.generated_source_path() {
            parts.push(format!("source: {path}"));
        }
        Self(parts.join(", "))
    }
}
