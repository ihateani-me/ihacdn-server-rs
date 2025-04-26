use askama::{FastWritable, Template};
use axum::{http::StatusCode, response::IntoResponse};

pub struct TemplateIndexRetention {
    pub min_age: String,
    pub max_age: String,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct TemplateIndex {
    pub https_mode: bool,
    pub hostname: String,
    pub filesize_limit: Option<String>,
    pub blacklist_extensions: Vec<String>,
    pub blacklist_ctypes: Vec<String>,
    pub file_retention: Option<TemplateIndexRetention>,
}

pub struct TemplateCodeData(String);

impl TemplateCodeData {
    pub fn new(code_data: String) -> Self {
        TemplateCodeData(code_data)
    }
}

impl std::fmt::Display for TemplateCodeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FastWritable for TemplateCodeData {
    fn write_into<W: core::fmt::Write + ?Sized>(
        &self,
        dest: &mut W,
        _values: &dyn askama::Values,
    ) -> askama::Result<()> {
        dest.write_str(&self.0)?;
        Ok(())
    }
}

impl askama::filters::HtmlSafe for TemplateCodeData {}

#[derive(Template)]
#[template(path = "paste.html")]
pub struct TemplatePaste {
    pub code_type: String,
    pub code_data: TemplateCodeData,
    pub file_id: String,
}

pub struct HtmlTemplate<T>(T);

impl<T> HtmlTemplate<T>
where
    T: Template,
{
    pub fn new(template: T) -> Self {
        HtmlTemplate(template)
    }
}

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> axum::response::Response {
        match self.0.render() {
            Ok(html) => axum::response::Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template. Error: {err}"),
            )
                .into_response(),
        }
    }
}
