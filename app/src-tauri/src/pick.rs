//! Modo apontar (F12.2): a página do preview manda o pick por uma navegação
//! `assistente-pick:` — a página remota não usa IPC nem CORS, só navega.
//!
//! O plugin do Tauri intercepta essa navegação, valida o payload, troca a URL que
//! veio no payload pela URL real da webview (a da página não é confiável) e emite o
//! evento `preview-pick` para a janela principal. O script injetado na webview fica
//! em `pick.js` (fora do bundle do projeto).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tauri::plugin::{Builder as PluginBuilder, TauriPlugin};
use tauri::webview::PageLoadPayload;
use tauri::{Emitter, Manager, Runtime, Url, Webview};

/// Esquema da navegação que carrega o pick da página do preview.
pub const PICK_SCHEME: &str = "assistente-pick";

/// Evento que leva o pick até a janela principal.
pub const PICK_EVENT: &str = "preview-pick";

/// Janela que recebe o pick.
const MAIN_WINDOW: &str = "main";

/// Payload codificado máximo aceito: acima disso o pick é descartado.
pub const MAX_PAYLOAD: usize = 64 * 1024;

/// Script injetado na webview do preview (fora do bundle do projeto).
/// O extrator do contexto de código (F12.3, o mesmo arquivo que a UI testa) vem antes do
/// `pick.js`, que chama `window.__assistenteCodeContext.extract`.
pub const SCRIPT: &str = concat!(
    include_str!("../../src/features/pick/codeContext.js"),
    "\n",
    include_str!("pick.js")
);

/// Retângulo do elemento na viewport (px de CSS).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Tamanho da janela da página.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
}

/// Quanto a página já rolou.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Scroll {
    pub x: f64,
    pub y: f64,
}

/// Contexto de código do elemento apontado (F12.3).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PickCodeContext {
    /// A página usa React?
    pub react: bool,
    /// Componente mais próximo do elemento.
    pub component: Option<String>,
    /// Pilha de componentes, do mais interno ao mais externo.
    pub stack: Vec<String>,
    /// Arquivo e linha do JSX (null em build de produção).
    pub source: Option<PickSource>,
}

/// Arquivo e linha do JSX (F12.3).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PickSource {
    /// Caminho do arquivo (como vem da página: `/src/...` ou URL do dev server).
    pub file: String,
    /// Linha do JSX.
    pub line: Option<i64>,
    /// Coluna do JSX.
    pub column: Option<i64>,
}

/// Elemento apontado na página do preview.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PickedElement {
    /// Tag do elemento (`button`, `div`…).
    pub tag: String,
    /// `id` do elemento, se tiver.
    pub id: Option<String>,
    /// Classes do elemento.
    pub classes: Vec<String>,
    /// `role` do elemento, se tiver.
    pub role: Option<String>,
    /// Atributos `aria-*` (o nome vem sem o prefixo: `label`, `describedby`…).
    pub aria: BTreeMap<String, String>,
    /// Texto curto do elemento.
    pub text: Option<String>,
    /// Trecho de HTML do elemento.
    pub html: Option<String>,
    /// Seletor CSS estável (id, classes ou caminho).
    pub selector: String,
    /// Caminho do elemento no DOM.
    pub dom_path: String,
    /// Retângulo do elemento na viewport.
    pub rect: Rect,
    /// Tamanho da janela da página.
    pub viewport: Viewport,
    /// Rolagem da página.
    pub scroll: Scroll,
    /// URL real da webview: o Rust troca pela URL carregada, a do payload não vale.
    pub url: String,
    /// Contexto de código (F12.3).
    pub code: Option<PickCodeContext>,
}

/// O que o Rust faz com uma navegação.
#[derive(Debug, PartialEq)]
pub enum Navigation {
    /// Não é um pick: a navegação continua.
    Other,
    /// Pick válido: emitir para o app e cancelar a navegação.
    Pick(Box<PickedElement>),
    /// Pick quebrado (payload grande demais ou inválido): cancelar e avisar o motivo.
    Broken(String),
}

/// Interpreta a URL de navegação. `Ok(None)` = não é um pick.
///
/// O payload é o JSON do pick codificado com `encodeURIComponent` (o que o script
/// faz). Payload grande demais ou quebrado é erro: o pick é descartado.
pub fn parse_pick(url: &str) -> Result<Option<PickedElement>, String> {
    let Some(rest) = url.strip_prefix(PICK_SCHEME) else {
        return Ok(None);
    };
    // O esquema vem colado do payload: "assistente-pick:o-payload".
    let Some(payload) = rest.strip_prefix(':') else {
        return Ok(None);
    };
    if payload.len() > MAX_PAYLOAD {
        return Err(format!(
            "payload do pick grande demais ({} bytes, o limite é {MAX_PAYLOAD})",
            payload.len()
        ));
    }
    let decoded = percent_decode(payload)?;
    let pick: PickedElement =
        serde_json::from_str(&decoded).map_err(|e| format!("payload do pick inválido: {e}"))?;
    Ok(Some(pick))
}

/// Troca a URL do payload pela URL real da webview: a página remota não escolhe a URL.
pub fn attach_real_url(mut pick: PickedElement, real_url: &str) -> PickedElement {
    pick.url = real_url.to_owned();
    pick
}

/// Decodifica `%XX` (o que o `encodeURIComponent` do script produz).
fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = input
                .get(index + 1..index + 3)
                .ok_or_else(|| "codificação porcentual incompleta".to_owned())?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| "codificação porcentual inválida".to_owned())?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "payload do pick não é UTF-8".to_owned())
}

/// Lê a navegação e diz o que fazer: o pick a emitir e se a navegação é cancelada.
///
/// A URL real da webview entra no lugar da URL do payload: a página remota não pode
/// escolher a URL que o app mostra.
pub fn intercept(url: &str, real_url: &str) -> Navigation {
    match parse_pick(url) {
        Ok(None) => Navigation::Other,
        Ok(Some(pick)) => Navigation::Pick(Box::new(attach_real_url(pick, real_url))),
        Err(e) => Navigation::Broken(e),
    }
}

/// Plugin do modo apontar: injeta o script na webview e intercepta a navegação do pick.
///
/// O script entra em toda webview (o próprio decide se serve, olhando o rótulo da
/// webview), então o preview funciona sem depender de quem o criou. A URL real de
/// cada webview é a da última página carregada: o `on_navigation` não pode perguntar
/// pela URL da webview (a thread do WebKit está ocupada com a navegação).
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let urls: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let loaded = urls.clone();
    let navigated = urls;
    PluginBuilder::new("pick")
        .js_init_script(SCRIPT)
        .on_page_load(move |webview: &Webview<R>, payload: &PageLoadPayload<'_>| {
            if let Ok(mut cache) = loaded.lock() {
                cache.insert(webview.label().to_owned(), payload.url().to_string());
            }
        })
        .on_navigation(move |webview: &Webview<R>, url: &Url| {
            if url.scheme() != PICK_SCHEME {
                return true;
            }
            let real_url = navigated
                .lock()
                .ok()
                .and_then(|cache| cache.get(webview.label()).cloned())
                .unwrap_or_default();
            match intercept(url.as_str(), &real_url) {
                // O pick não vira uma página: a navegação é sempre cancelada.
                Navigation::Other => true,
                Navigation::Pick(pick) => {
                    let _ = webview.app_handle().emit_to(MAIN_WINDOW, PICK_EVENT, *pick);
                    false
                }
                Navigation::Broken(e) => {
                    eprintln!("apontar: {e}");
                    false
                }
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Como o `encodeURIComponent` do script.
    fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'!'
                | b'~'
                | b'*'
                | b'\''
                | b'('
                | b')' => (byte as char).to_string(),
                _ => format!("%{byte:02X}"),
            })
            .collect()
    }

    fn payload() -> String {
        r##"{"tag":"button","id":"save","classes":["btn","primary"],"role":null,"aria":{"label":"Salvar"},"text":"Salvar","html":"<button id=\"save\">Salvar</button>","selector":"#save","dom_path":"body > main > button#save","rect":{"x":10,"y":20,"width":120,"height":32},"viewport":{"width":1280,"height":720},"scroll":{"x":0,"y":140},"url":"http://mentira.local/","code":{"react":true,"component":"Botao","stack":["Botao","App"],"source":{"file":"/src/features/x/Botao.tsx","line":42,"column":7}}}"##
            .to_owned()
    }

    fn navigation(payload: &str) -> String {
        format!("{PICK_SCHEME}:{}", encode(payload))
    }

    #[test]
    fn le_o_pick_e_troca_a_url_do_payload_pela_url_real() {
        let Navigation::Pick(pick) = intercept(&navigation(&payload()), "http://localhost:5173/")
        else {
            panic!("esperava um pick");
        };
        assert_eq!(pick.tag, "button");
        assert_eq!(pick.id.as_deref(), Some("save"));
        assert_eq!(pick.classes, vec!["btn", "primary"]);
        assert_eq!(pick.aria.get("label").map(String::as_str), Some("Salvar"));
        assert_eq!(pick.text.as_deref(), Some("Salvar"));
        assert_eq!(pick.selector, "#save");
        assert_eq!(pick.dom_path, "body > main > button#save");
        assert_eq!(pick.rect.width, 120.0);
        assert_eq!(pick.viewport.height, 720.0);
        assert_eq!(pick.scroll.y, 140.0);
        // A URL do payload não vale: a real da webview entra no lugar.
        assert_eq!(pick.url, "http://localhost:5173/");
        // O contexto de código (F12.3) passa adiante como veio da página.
        let code = pick.code.expect("contexto de código");
        assert_eq!(code.component.as_deref(), Some("Botao"));
        assert_eq!(code.stack, vec!["Botao", "App"]);
        let source = code.source.expect("fonte");
        assert_eq!(source.file, "/src/features/x/Botao.tsx");
        assert_eq!(source.line, Some(42));
    }

    #[test]
    fn o_script_injeta_o_extrator_antes_do_apontar() {
        let extractor = SCRIPT
            .find("__assistenteCodeContext =")
            .expect("extrator no script");
        let pick = SCRIPT.find("__assistentePick").expect("apontar no script");
        assert!(extractor < pick, "o extrator tem que vir antes do pick.js");
    }

    #[test]
    fn uma_navegação_comum_não_e_pick() {
        assert_eq!(parse_pick("http://localhost:5173/").unwrap(), None);
        assert_eq!(parse_pick("assistente-pickoutro://x").unwrap(), None);
        assert_eq!(
            intercept("http://localhost:5173/", "http://localhost:5173/"),
            Navigation::Other
        );
    }

    #[test]
    fn payload_acima_do_limite_e_descartado() {
        let url = format!("{PICK_SCHEME}:{}", "a".repeat(MAX_PAYLOAD + 1));
        let Navigation::Broken(error) = intercept(&url, "http://localhost:5173/") else {
            panic!("esperava um pick quebrado");
        };
        assert!(error.contains("grande demais"), "{error}");
    }

    #[test]
    fn payload_quebrado_e_erro() {
        let Navigation::Broken(error) = intercept(&navigation("{não é json"), "http://x/") else {
            panic!("esperava um pick quebrado");
        };
        assert!(error.contains("inválido"), "{error}");
    }

    #[test]
    fn a_decodificação_preserva_o_acento() {
        let Navigation::Pick(pick) = intercept(
            &navigation(r#"{"tag":"button","text":"Salvar ação"}"#),
            "http://localhost:5173/",
        ) else {
            panic!("esperava um pick");
        };
        assert_eq!(pick.text.as_deref(), Some("Salvar ação"));
    }

    #[test]
    fn a_url_da_webview_vazia_nao_derruba_o_pick() {
        // A webview ainda não carregou página: o pick vem sem URL, mas não quebra.
        let Navigation::Pick(pick) = intercept(&navigation(&payload()), "") else {
            panic!("esperava um pick");
        };
        assert_eq!(pick.url, "");
    }
}
