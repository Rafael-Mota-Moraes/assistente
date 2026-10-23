//! Apontar e pedir na tela (F12.4): o detalhe do pick, o prompt curto que o agente recebe e
//! o texto do card do quadro. Puro: quem escreve o arquivo e cola no terminal é o hubd.
//! Spec: docs/specs/03-interface.md

use crate::kanban::Card;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Onde o pedido vai (F12.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickTarget {
    /// Cola no prompt de uma sessão de agente (sem Enter: o Enter é do usuário).
    #[default]
    Sessao,
    /// Vira card no backlog do projeto (F11.1).
    Quadro,
}

/// Pasta (dentro do projeto) onde ficam os arquivos dos picks.
pub const PICKS_DIR: &str = ".assistente/picks";
/// O que vai para o `info/exclude` do repo: os picks são registro local, não entram no git
/// (como o `.ai-jail`, F0.5).
pub const PICKS_EXCLUDE: &str = "/.assistente/picks";
/// Tamanho máximo do texto do pedido (o resto é cortado no prompt).
const MAX_REQUEST: usize = 600;
/// Tamanho máximo do texto do elemento no prompt.
const MAX_TEXT: usize = 120;

/// O elemento apontado na página (F12.2) e o contexto de código (F12.3).
///
/// Os campos que o app mandar e este módulo não conhecer ficam em `extra`: nada do pick se
/// perde no arquivo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PickDetail {
    /// URL real da webview (o hubd não confia na URL do payload).
    pub url: String,
    /// Seletor CSS estável do elemento.
    pub selector: String,
    /// Texto curto do elemento.
    pub text: String,
    /// Trecho de HTML do elemento.
    pub html: String,
    /// Tag do elemento (`button`, `div`…).
    pub tag: String,
    /// `id` do elemento.
    pub id: Option<String>,
    /// Classes do elemento.
    pub classes: Vec<String>,
    /// `role` do elemento.
    pub role: Option<String>,
    /// `aria-label` do elemento.
    pub aria_label: Option<String>,
    /// Caminho no DOM.
    pub dom_path: Option<String>,
    /// Componente React (F12.3).
    pub component: Option<String>,
    /// `arquivo:linha` do fonte do componente (F12.3).
    pub source: Option<String>,
    /// Pilha de componentes (F12.3).
    pub components: Vec<String>,
    /// Campos que o app mandar e este módulo não conhecer.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// O pedido que o app manda (F12.4).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PickRequest {
    /// Alvo escolhido no modal.
    pub target: PickTarget,
    /// Sessão que recebe o pedido (alvo = sessão).
    pub session: Option<String>,
    /// O recado do usuário.
    pub request: String,
    /// O detalhe do pick.
    pub pick: PickDetail,
}

/// O que a rota devolve (F12.4).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PickResult {
    /// Onde o pedido foi entregue.
    pub target: PickTarget,
    /// A sessão que recebeu o pedido (alvo = sessão).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// O card criado no quadro (alvo = quadro).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
    /// O prompt que o agente recebeu.
    pub prompt: String,
    /// O arquivo do pick, dentro do projeto.
    pub file: String,
}

impl PickRequest {
    /// O recado não pode ser vazio: sem ele o agente não sabe o que fazer.
    pub fn validate(&self) -> Result<(), String> {
        if self.request.trim().is_empty() {
            return Err("o pedido está vazio: escreva o que quer mudar".into());
        }
        Ok(())
    }

    /// O prompt curto que o agente recebe: uma linha com URL, onde, seletor, texto e pedido.
    ///
    /// Uma linha só: o texto é colado no prompt do agente sem Enter, e um `\n` no meio
    /// entraria como Enter (F12.4).
    pub fn prompt(&self) -> String {
        let mut out = String::from("Apontei ");
        if let Some(url) = clean(&self.pick.url) {
            out.push_str(&format!("na página {url} "));
        }
        out.push_str(&format!("o elemento {} ", self.pick.describe()));
        if let Some(place) = self.pick.place() {
            out.push_str(&format!("em {place} "));
        }
        out.push_str(&format!(
            "Pedido: {}.",
            one_line(&self.request, MAX_REQUEST)
        ));
        if self.pick.source.is_none() {
            // Build de produção: o fonte não aparece (F12.3). O agente procura antes de editar.
            out.push(' ');
            out.push_str(&self.pick.search_hint());
        }
        out
    }

    /// Título do card: o pedido em uma linha.
    pub fn card_title(&self) -> String {
        let text = one_line(&self.request, 90);
        if text.is_empty() {
            "Pedido apontado na tela".to_owned()
        } else {
            text
        }
    }

    /// Descrição do card: autossuficiente, porque o clone do notebook não tem o arquivo do pick.
    pub fn card_description(&self) -> String {
        let mut out = String::from("Pedido feito apontando um elemento na página do projeto.\n\n");
        if let Some(url) = clean(&self.pick.url) {
            out.push_str(&format!("- URL: {url}\n"));
        }
        if let Some(place) = self.pick.place() {
            out.push_str(&format!("- Onde: {place}\n"));
        }
        if let Some(component) = clean_opt(self.pick.component.as_deref()) {
            out.push_str(&format!("- Componente: {component}\n"));
        }
        if !self.pick.components.is_empty() {
            out.push_str(&format!(
                "- Componentes: {}\n",
                self.pick.components.join(" > ")
            ));
        }
        if let Some(selector) = clean(&self.pick.selector) {
            out.push_str(&format!("- Seletor: {selector}\n"));
        }
        if let Some(text) = clean(&self.pick.text) {
            out.push_str(&format!("- Texto: {}\n", one_line(text, MAX_TEXT)));
        }
        if let Some(html) = clean(&self.pick.html) {
            out.push_str(&format!("- HTML: {}\n", one_line(html, MAX_TEXT)));
        }
        if self.pick.source.is_none() {
            out.push_str(&format!("- {}\n", self.pick.search_hint()));
        }
        out.push_str(&format!("\nPedido: {}", self.request.trim()));
        out
    }

    /// O arquivo do pick: o detalhe inteiro, para o agente consultar (F12.4).
    pub fn markdown(&self, stamp: &str) -> String {
        let mut out = String::from("# Pick apontado na tela\n\n");
        out.push_str(&format!("- Data: {stamp}\n"));
        if let Some(url) = clean(&self.pick.url) {
            out.push_str(&format!("- URL: {url}\n"));
        }
        if let Some(place) = self.pick.place() {
            out.push_str(&format!("- Onde: {place}\n"));
        }
        if let Some(component) = clean_opt(self.pick.component.as_deref()) {
            out.push_str(&format!("- Componente: {component}\n"));
        }
        if !self.pick.components.is_empty() {
            out.push_str(&format!(
                "- Componentes: {}\n",
                self.pick.components.join(" > ")
            ));
        }
        if let Some(selector) = clean(&self.pick.selector) {
            out.push_str(&format!("- Seletor: {selector}\n"));
        }
        if let Some(tag) = clean(&self.pick.tag) {
            out.push_str(&format!("- Elemento: <{tag}>\n"));
        }
        if let Some(text) = clean(&self.pick.text) {
            out.push_str(&format!("- Texto: {}\n", one_line(text, MAX_TEXT)));
        }
        if let Some(html) = clean(&self.pick.html) {
            out.push_str(&format!("- HTML: {}\n", one_line(html, MAX_TEXT)));
        }
        out.push_str(&format!("\n## Pedido\n\n{}\n", self.request.trim()));
        out.push_str(&format!("\n## Prompt\n\n{}\n", self.prompt()));
        out.push_str("\n## Pick (JSON)\n\n```json\n");
        out.push_str(&serde_json::to_string_pretty(&self.pick).unwrap_or_else(|_| "{}".to_owned()));
        out.push_str("\n```\n");
        out
    }
}

impl PickDetail {
    /// Uma linha que descreve o elemento: seletor e texto.
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(selector) = clean(&self.selector) {
            parts.push(selector.to_owned());
        }
        if let Some(text) = clean(&self.text) {
            parts.push(format!("\"{}\"", one_line(text, MAX_TEXT)));
        }
        if parts.is_empty() {
            parts.push("apontado".to_owned());
        }
        parts.join(" ")
    }

    /// Sem `arquivo:linha` (build de produção, F12.3): o agente procura o arquivo antes de
    /// editar, com as pistas que o pick tem (texto, classe, componente), em vez de chutar.
    pub fn search_hint(&self) -> String {
        let mut hints: Vec<String> = Vec::new();
        if let Some(text) = clean(&self.text) {
            hints.push(format!("pelo texto \"{}\"", one_line(text, MAX_TEXT)));
        }
        // A classe que o próprio apontar põe no elemento não ajuda a achar o arquivo.
        if let Some(class) = self
            .classes
            .iter()
            .map(|c| c.trim())
            .find(|c| !c.is_empty() && !c.starts_with("assistente-"))
        {
            hints.push(format!("pela classe \"{class}\""));
        }
        if let Some(component) = clean_opt(self.component.as_deref()) {
            hints.push(format!("pelo componente {component}"));
        }
        let how = if hints.is_empty() {
            "pelo seletor".to_owned()
        } else {
            hints.join(", ")
        };
        format!(
            "Sem a origem do componente: antes de editar, procure o arquivo no projeto \
             (grep {how}) e confirme o caminho; não chute o arquivo."
        )
    }

    /// Onde o elemento está: o `arquivo:linha` (F12.3), o componente ou o caminho no DOM.
    pub fn place(&self) -> Option<String> {
        clean_opt(self.source.as_deref())
            .or_else(|| clean_opt(self.component.as_deref()))
            .or_else(|| clean_opt(self.dom_path.as_deref()))
            .map(str::to_owned)
    }
}

/// Texto vazio (ou só espaços) = nada.
fn clean(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Como `clean`, para quem já tem um `Option`.
fn clean_opt(text: Option<&str>) -> Option<&str> {
    text.and_then(clean)
}

/// Uma linha só: quebras e espaços viram um espaço, e o texto longo é cortado.
fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        let cut: String = flat.chars().take(max).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail() -> PickDetail {
        PickDetail {
            url: "http://localhost:5173".into(),
            selector: "button.salvar".into(),
            text: "Salvar".into(),
            html: "<button class=\"salvar\">Salvar</button>".into(),
            tag: "button".into(),
            component: Some("Botao".into()),
            source: Some("src/features/x/Botao.tsx:42".into()),
            components: vec!["App".into(), "Botao".into()],
            ..Default::default()
        }
    }

    fn request() -> PickRequest {
        PickRequest {
            target: PickTarget::Sessao,
            session: Some("pessoal--game--claude-1".into()),
            request: "deixa verde".into(),
            pick: detail(),
        }
    }

    #[test]
    fn prompt_has_url_place_selector_text_and_request_in_one_line() {
        let prompt = request().prompt();
        assert!(prompt.contains("http://localhost:5173"), "{prompt}");
        assert!(prompt.contains("src/features/x/Botao.tsx:42"), "{prompt}");
        assert!(prompt.contains("button.salvar"), "{prompt}");
        assert!(prompt.contains("\"Salvar\""), "{prompt}");
        assert!(prompt.contains("Pedido: deixa verde."), "{prompt}");
        assert!(!prompt.contains('\n'), "{prompt}");
        // Com o fonte, o agente não precisa procurar.
        assert!(!prompt.contains("procure o arquivo"), "{prompt}");
    }

    #[test]
    fn prompt_without_source_asks_the_agent_to_look_before_editing() {
        let mut body = request();
        body.pick.source = None;
        let prompt = body.prompt();
        assert!(prompt.contains("procure o arquivo"), "{prompt}");
        assert!(prompt.contains("não chute"), "{prompt}");
        assert!(!prompt.contains('\n'), "{prompt}");
    }

    #[test]
    fn search_hint_uses_text_class_and_component() {
        let mut pick = detail();
        pick.classes = vec!["assistente-pick-hover".into(), "salvar".into()];
        let hint = pick.search_hint();
        assert!(hint.contains("grep pelo texto \"Salvar\""), "{hint}");
        assert!(hint.contains("pela classe \"salvar\""), "{hint}");
        assert!(!hint.contains("assistente-"), "{hint}");
        assert!(hint.contains("pelo componente Botao"), "{hint}");
    }

    #[test]
    fn search_hint_without_clues_falls_back_to_the_selector() {
        let pick = PickDetail {
            selector: "div > span".into(),
            ..Default::default()
        };
        assert!(pick.search_hint().contains("grep pelo seletor"));
    }

    #[test]
    fn request_with_breaks_becomes_one_line() {
        let mut body = request();
        body.request = "deixa\nverde\n\n".into();
        let prompt = body.prompt();
        assert!(!prompt.contains('\n'), "{prompt}");
        assert!(prompt.contains("Pedido: deixa verde."), "{prompt}");
    }

    #[test]
    fn long_request_is_cut() {
        let mut body = request();
        body.request = "a".repeat(MAX_REQUEST + 50);
        let prompt = body.prompt();
        assert!(prompt.contains('…'), "{prompt}");
        assert!(prompt.len() < MAX_REQUEST + 200, "{}", prompt.len());
    }

    #[test]
    fn empty_request_is_an_error() {
        let mut body = request();
        body.request = "   ".into();
        assert!(body.validate().is_err());
        body.request = "deixa verde".into();
        assert!(body.validate().is_ok());
    }

    #[test]
    fn card_description_does_not_need_the_local_file() {
        let body = request();
        let description = body.card_description();
        assert!(
            description.contains("URL: http://localhost:5173"),
            "{description}"
        );
        assert!(
            description.contains("Onde: src/features/x/Botao.tsx:42"),
            "{description}"
        );
        assert!(
            description.contains("Seletor: button.salvar"),
            "{description}"
        );
        assert!(description.contains("Texto: Salvar"), "{description}");
        assert!(description.contains("Pedido: deixa verde"), "{description}");
        assert!(!description.contains(".assistente/picks"), "{description}");
        assert_eq!(body.card_title(), "deixa verde");
    }

    #[test]
    fn card_description_without_source_says_to_look_first() {
        let mut body = request();
        body.pick.source = None;
        assert!(body.card_description().contains("procure o arquivo"));
    }

    #[test]
    fn markdown_keeps_the_whole_pick() {
        let mut body = request();
        body.pick
            .extra
            .insert("meu_campo".into(), serde_json::json!("valor"));
        let text = body.markdown("2026-10-22-11-44-05");
        assert!(text.contains("2026-10-22-11-44-05"), "{text}");
        assert!(text.contains("src/features/x/Botao.tsx:42"), "{text}");
        assert!(text.contains("deixa verde"), "{text}");
        assert!(text.contains("meu_campo"), "{text}");
    }

    #[test]
    fn pick_reads_a_body_without_the_optional_fields() {
        let body: PickRequest = serde_json::from_str(
            r#"{"target":"quadro","request":"deixa verde","pick":{"url":"http://x","selector":"b"}}"#,
        )
        .unwrap();
        assert_eq!(body.target, PickTarget::Quadro);
        assert_eq!(body.pick.url, "http://x");
        assert!(body.pick.text.is_empty());
        assert_eq!(body.pick.describe(), "b");
    }

    #[test]
    fn target_reads_the_portuguese_names() {
        let sessao: PickTarget = serde_json::from_str("\"sessao\"").unwrap();
        let quadro: PickTarget = serde_json::from_str("\"quadro\"").unwrap();
        assert_eq!(sessao, PickTarget::Sessao);
        assert_eq!(quadro, PickTarget::Quadro);
        assert!(serde_json::from_str::<PickTarget>("\"outro\"").is_err());
    }
}
