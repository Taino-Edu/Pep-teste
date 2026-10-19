//! API de consulta de CPF/nome em planilha Excel.
//!
//! Rodar:  cargo run --release
//! Config (variáveis de ambiente, opcionais): PLANILHA, COLUNA_CPF, COLUNA_NOME, PORTA
mod comparador;
mod planilha;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use comparador::{consultar, RespostaConsulta, Resultado};
use planilha::{achar_coluna, ler_tabela, Planilha, EXTENSOES};

const PASTA_DADOS: &str = "dados";

struct Estado {
    planilha: Option<Planilha>,
    erro: Option<String>,
}

type App = Arc<RwLock<Estado>>;
type Erro = (StatusCode, Json<Value>);

fn erro(status: StatusCode, msg: impl Into<String>) -> Erro {
    (status, Json(json!({ "detail": msg.into() })))
}

fn extensao(nome: &str) -> String {
    Path::new(nome).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

/// Carrega a planilha de PLANILHA (variável de ambiente) ou a mais recente em ./dados.
fn carregar(caminho: Option<PathBuf>) -> Estado {
    let caminho = caminho.or_else(|| std::env::var("PLANILHA").ok().map(PathBuf::from)).or_else(|| {
        std::fs::read_dir(PASTA_DADOS)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| EXTENSOES.contains(&extensao(&p.to_string_lossy()).as_str()))
            .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
    });
    let Some(caminho) = caminho else {
        return Estado {
            planilha: None,
            erro: Some(format!("Nenhuma planilha em ./{PASTA_DADOS}. Envie uma em POST /planilha.")),
        };
    };
    let col_cpf = std::env::var("COLUNA_CPF").ok();
    let col_nome = std::env::var("COLUNA_NOME").ok();
    match Planilha::abrir(&caminho, col_cpf.as_deref(), col_nome.as_deref()) {
        Ok(p) => Estado { planilha: Some(p), erro: None },
        Err(e) => Estado { planilha: None, erro: Some(format!("Erro ao ler {}: {e}", caminho.display())) },
    }
}

fn com_planilha<T>(app: &App, f: impl FnOnce(&Planilha) -> Result<T, Erro>) -> Result<T, Erro> {
    let estado = app.read().unwrap();
    match &estado.planilha {
        Some(p) => f(p),
        None => Err(erro(StatusCode::SERVICE_UNAVAILABLE, estado.erro.clone().unwrap_or_default())),
    }
}

/// Lê o campo "arquivo" de um upload multipart.
async fn receber_arquivo(mut multipart: Multipart) -> Result<(String, Vec<u8>), Erro> {
    while let Some(campo) = multipart.next_field().await.map_err(|e| erro(StatusCode::BAD_REQUEST, e.to_string()))? {
        if campo.name() == Some("arquivo") {
            let nome = campo.file_name().unwrap_or("planilha.xlsx").to_string();
            if !EXTENSOES.contains(&extensao(&nome).as_str()) {
                return Err(erro(StatusCode::BAD_REQUEST, "Envie um arquivo .xlsx, .xls ou .csv"));
            }
            let bytes = campo.bytes().await.map_err(|e| erro(StatusCode::BAD_REQUEST, e.to_string()))?;
            return Ok((nome, bytes.to_vec()));
        }
    }
    Err(erro(StatusCode::BAD_REQUEST, "Envie o arquivo no campo 'arquivo'"))
}

async fn info_planilha(State(app): State<App>) -> Result<Json<planilha::Info>, Erro> {
    com_planilha(&app, |p| Ok(Json(p.info())))
}

async fn enviar_planilha(State(app): State<App>, multipart: Multipart) -> Result<Json<planilha::Info>, Erro> {
    let (nome, bytes) = receber_arquivo(multipart).await?;
    let destino = Path::new(PASTA_DADOS).join(Path::new(&nome).file_name().unwrap_or_default());
    std::fs::write(&destino, bytes).map_err(|e| erro(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let novo = carregar(Some(destino));
    *app.write().unwrap() = novo;
    info_planilha(State(app)).await
}

#[derive(Deserialize)]
struct ParamsConsulta {
    cpf: Option<String>,
    nome: Option<String>,
    similaridade_minima: Option<f64>,
}

/// Registro em que o CPF e o nome batem com a lista PEP.
#[derive(Serialize)]
struct Igual {
    #[serde(skip_serializing_if = "Option::is_none")]
    linha: Option<usize>,
    cpf: Option<String>,
    nome: Option<String>,
    cpf_pep: Option<String>,
    nome_pep: Option<String>,
}

/// Só os registros com CPF e nome iguais.
fn iguais(r: RespostaConsulta, linha: Option<usize>) -> impl Iterator<Item = Igual> {
    let (cpf, nome) = (r.consulta.cpf, r.consulta.nome);
    r.encontrados.into_iter().filter(|e| e.resultado == Resultado::CpfENome).map(move |e| Igual {
        linha,
        cpf: cpf.clone(),
        nome: nome.clone(),
        cpf_pep: e.cpf_planilha,
        nome_pep: e.nome_planilha,
    })
}

async fn consulta(State(app): State<App>, Query(q): Query<ParamsConsulta>) -> Result<Json<Value>, Erro> {
    com_planilha(&app, |p| {
        let r = consultar(p, q.cpf.as_deref(), q.nome.as_deref(), q.similaridade_minima.unwrap_or(0.85))
            .map_err(|e| erro(StatusCode::BAD_REQUEST, e))?;
        let lista: Vec<Igual> = iguais(r, None).collect();
        Ok(Json(json!({ "total_iguais": lista.len(), "iguais": lista })))
    })
}

#[derive(Deserialize)]
struct ParamsCruzar {
    similaridade_minima: Option<f64>,
}

/// Cruza uma lista inteira (.xlsx/.csv com colunas de CPF e nome) com a planilha carregada.
async fn cruzar(
    State(app): State<App>,
    Query(q): Query<ParamsCruzar>,
    multipart: Multipart,
) -> Result<Json<Value>, Erro> {
    let (nome_arquivo, bytes) = receber_arquivo(multipart).await?;
    let lista = ler_tabela(bytes, &extensao(&nome_arquivo)).map_err(|e| erro(StatusCode::BAD_REQUEST, e))?;
    let col_cpf = achar_coluna(&lista.colunas, None, "CPF").map_err(|e| erro(StatusCode::BAD_REQUEST, e))?;
    let col_nome = achar_coluna(&lista.colunas, None, "NOME").map_err(|e| erro(StatusCode::BAD_REQUEST, e))?;
    let similaridade = q.similaridade_minima.unwrap_or(0.85);

    com_planilha(&app, |base| {
        let mut encontrados = Vec::new();
        let mut com_erro = 0;
        for (i, row) in lista.linhas.iter().enumerate() {
            let cpf = row.get(col_cpf).cloned().flatten();
            let nome = row.get(col_nome).cloned().flatten();
            match consultar(base, cpf.as_deref(), nome.as_deref(), similaridade) {
                Ok(r) => encontrados.extend(iguais(r, Some(i + 2))),
                Err(_) => com_erro += 1,
            }
        }
        Ok(Json(json!({
            "total_consultados": lista.linhas.len(),
            "total_iguais": encontrados.len(),
            "linhas_com_erro": com_erro,
            "iguais": encontrados,
        })))
    })
}

#[tokio::main]
async fn main() {
    std::fs::create_dir_all(PASTA_DADOS).ok();
    let estado = carregar(None);
    match (&estado.planilha, &estado.erro) {
        (Some(p), _) => println!("Planilha carregada: {} ({} linhas)", p.caminho.display(), p.registros.len()),
        (None, Some(e)) => println!("Aviso: {e}"),
        _ => {}
    }

    let app = Router::new()
        .route("/planilha", get(info_planilha).post(enviar_planilha))
        .route("/consulta", get(consulta))
        .route("/cruzar", axum::routing::post(cruzar))
        .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
        .with_state(Arc::new(RwLock::new(estado)));

    let porta = std::env::var("PORTA").unwrap_or_else(|_| "8000".into());
    let endereco = format!("127.0.0.1:{porta}");
    let listener = tokio::net::TcpListener::bind(&endereco).await.expect("não consegui abrir a porta");
    println!("API rodando em http://{endereco}");
    axum::serve(listener, app).await.unwrap();
}
