//! Leitura da planilha (Excel/CSV), normalização de CPF/nome e índices de busca.
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use calamine::{open_workbook_auto_from_rs, Data, Reader};
use serde::Serialize;
use serde_json::{Map, Value};

pub const EXTENSOES: &[&str] = &["xlsx", "xlsm", "xls", "csv"];
const PALAVRAS_IGNORADAS: &[&str] = &["DE", "DA", "DO", "DAS", "DOS", "E"];

/// Maiúsculas, sem acentos, sem pontuação e com espaços simples.
pub fn normalizar_nome(texto: &str) -> String {
    let sem_acento: String = deunicode::deunicode(texto)
        .chars()
        .map(|c| if c.is_ascii_alphabetic() { c.to_ascii_uppercase() } else { ' ' })
        .collect();
    sem_acento.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Devolve 11 caracteres: dígitos conhecidos e '*' nas posições ocultas.
///
/// Aceita CPF completo (com ou sem pontuação, com zeros à esquerda perdidos pelo
/// Excel) e CPF mascarado como `***.456.789-**`.
pub fn normalizar_cpf(valor: &str) -> Option<String> {
    let mut s = valor.trim().to_string();
    if let Some((inteiro, decimais)) = s.split_once('.') {
        // número que o Excel guardou como float: "12345678901.0"
        if !inteiro.is_empty() && inteiro.bytes().all(|b| b.is_ascii_digit()) && !decimais.is_empty() && decimais.bytes().all(|b| b == b'0') {
            s = inteiro.to_string();
        }
    }
    if s.contains(['e', 'E']) {
        // notação científica: "1.2345678901E+10"
        if let Ok(f) = s.parse::<f64>() {
            s = format!("{f:.0}");
        }
    }
    let s: String = s
        .chars()
        .filter_map(|c| match c {
            '0'..='9' | '*' => Some(c),
            'x' | 'X' | '#' => Some('*'),
            _ => None,
        })
        .collect();
    if !s.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let s = if !s.contains('*') && (9..11).contains(&s.len()) {
        format!("{s:0>11}") // zeros à esquerda que o Excel apaga
    } else {
        s
    };
    (s.len() == 11).then_some(s)
}

pub fn formatar_cpf(cpf: &str) -> String {
    format!("{}.{}.{}-{}", &cpf[..3], &cpf[3..6], &cpf[6..9], &cpf[9..])
}

pub fn palavras_nome(nome: &str) -> impl Iterator<Item = &str> {
    nome.split_whitespace().filter(|p| !PALAVRAS_IGNORADAS.contains(p))
}

pub struct Tabela {
    pub colunas: Vec<String>,
    pub linhas: Vec<Vec<Option<String>>>,
}

fn celula(d: &Data) -> Option<String> {
    let s = match d {
        Data::Empty => return None,
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        outro => outro.to_string(),
    };
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Lê .xlsx/.xls/.csv (a partir dos bytes). A primeira linha é o cabeçalho.
pub fn ler_tabela(bytes: Vec<u8>, extensao: &str) -> Result<Tabela, String> {
    let mut linhas: Vec<Vec<Option<String>>> = match extensao.to_lowercase().as_str() {
        "csv" => ler_csv(&bytes)?,
        "xlsx" | "xlsm" | "xls" => {
            let mut livro = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            let aba = livro
                .worksheet_range_at(0)
                .ok_or("A planilha não tem abas")?
                .map_err(|e| e.to_string())?;
            aba.rows().map(|r| r.iter().map(celula).collect()).collect()
        }
        outro => return Err(format!("Formato não suportado: .{outro} (use .xlsx, .xls ou .csv)")),
    };
    if linhas.is_empty() {
        return Err("A planilha está vazia".into());
    }
    let colunas = linhas
        .remove(0)
        .into_iter()
        .enumerate()
        .map(|(i, c)| c.unwrap_or_else(|| format!("Coluna{}", i + 1)))
        .collect();
    Ok(Tabela { colunas, linhas })
}

fn ler_csv(bytes: &[u8]) -> Result<Vec<Vec<Option<String>>>, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let texto = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(), // CSV do governo vem em latin-1
    };
    let primeira = texto.lines().next().unwrap_or("");
    let separador = [b';', b',', b'\t']
        .into_iter()
        .max_by_key(|&s| primeira.bytes().filter(|&b| b == s).count())
        .unwrap();
    csv::ReaderBuilder::new()
        .delimiter(separador)
        .has_headers(false)
        .flexible(true)
        .from_reader(texto.as_bytes())
        .records()
        .map(|r| {
            r.map(|r| r.iter().map(|c| Some(c.trim().to_string()).filter(|c| !c.is_empty())).collect())
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub fn achar_coluna(colunas: &[String], preferida: Option<&str>, termo: &str) -> Result<usize, String> {
    if let Some(p) = preferida {
        return colunas
            .iter()
            .position(|c| c == p)
            .ok_or_else(|| format!("Coluna '{p}' não existe. Colunas: {colunas:?}"));
    }
    colunas
        .iter()
        .position(|c| {
            let n = normalizar_nome(c);
            n.starts_with(termo) || n.split(' ').any(|p| p == termo)
        })
        .ok_or_else(|| format!("Não achei coluna de {termo} na planilha. Colunas: {colunas:?}"))
}

pub struct Registro {
    /// Linha no Excel (cabeçalho = linha 1)
    pub linha: usize,
    pub cpf_original: Option<String>,
    pub cpf: Option<String>,
    pub nome_original: Option<String>,
    pub nome: String,
    pub dados: Map<String, Value>,
}

#[derive(Serialize)]
pub struct Info {
    arquivo: String,
    total_linhas: usize,
    colunas: Vec<String>,
    coluna_cpf: String,
    coluna_nome: String,
    linhas_sem_cpf_valido: usize,
}

pub struct Planilha {
    pub caminho: PathBuf,
    pub colunas: Vec<String>,
    pub coluna_cpf: usize,
    pub coluna_nome: usize,
    pub registros: Vec<Registro>,
    /// CPFs agrupados pelo "formato" (posições visíveis) -> dígitos visíveis -> registros
    por_formato: HashMap<Vec<usize>, HashMap<String, Vec<usize>>>,
    por_palavra: HashMap<String, Vec<usize>>,
}

impl Planilha {
    pub fn abrir(caminho: &Path, coluna_cpf: Option<&str>, coluna_nome: Option<&str>) -> Result<Self, String> {
        let bytes = std::fs::read(caminho).map_err(|e| e.to_string())?;
        let ext = caminho.extension().and_then(|e| e.to_str()).unwrap_or("");
        let tabela = ler_tabela(bytes, ext)?;
        let coluna_cpf = achar_coluna(&tabela.colunas, coluna_cpf, "CPF")?;
        let coluna_nome = achar_coluna(&tabela.colunas, coluna_nome, "NOME")?;

        let registros = tabela
            .linhas
            .into_iter()
            .enumerate()
            .map(|(i, mut linha)| {
                linha.resize(tabela.colunas.len(), None);
                let cpf_original = linha[coluna_cpf].clone();
                let nome_original = linha[coluna_nome].clone();
                Registro {
                    linha: i + 2,
                    cpf: cpf_original.as_deref().and_then(normalizar_cpf),
                    nome: nome_original.as_deref().map(normalizar_nome).unwrap_or_default(),
                    cpf_original,
                    nome_original,
                    dados: tabela
                        .colunas
                        .iter()
                        .cloned()
                        .zip(linha.into_iter().map(|v| v.map_or(Value::Null, Value::String)))
                        .collect(),
                }
            })
            .collect();

        let mut planilha = Planilha {
            caminho: caminho.to_path_buf(),
            colunas: tabela.colunas,
            coluna_cpf,
            coluna_nome,
            registros,
            por_formato: HashMap::new(),
            por_palavra: HashMap::new(),
        };
        planilha.indexar();
        Ok(planilha)
    }

    fn indexar(&mut self) {
        for (i, r) in self.registros.iter().enumerate() {
            if let Some(cpf) = &r.cpf {
                let formato: Vec<usize> = cpf.bytes().enumerate().filter(|(_, c)| *c != b'*').map(|(p, _)| p).collect();
                let chave: String = formato.iter().map(|&p| cpf.as_bytes()[p] as char).collect();
                self.por_formato.entry(formato).or_default().entry(chave).or_default().push(i);
            }
            for palavra in palavras_nome(&r.nome).collect::<HashSet<_>>() {
                self.por_palavra.entry(palavra.to_string()).or_default().push(i);
            }
        }
    }

    pub fn candidatos_cpf(&self, cpf: &str) -> HashSet<usize> {
        let cpf = cpf.as_bytes();
        let mut encontrados = HashSet::new();
        for (formato, indice) in &self.por_formato {
            if formato.iter().all(|&p| cpf[p] != b'*') {
                let chave: String = formato.iter().map(|&p| cpf[p] as char).collect();
                encontrados.extend(indice.get(&chave).into_iter().flatten());
            } else {
                // consulta também mascarada: compara só as posições visíveis nos dois
                for (chave, linhas) in indice {
                    if formato.iter().zip(chave.bytes()).all(|(&p, d)| cpf[p] == b'*' || cpf[p] == d) {
                        encontrados.extend(linhas);
                    }
                }
            }
        }
        encontrados
    }

    pub fn candidatos_nome(&self, nome: &str) -> HashSet<usize> {
        // Usa as 2 palavras mais raras do nome para não varrer a planilha inteira
        let mut palavras: Vec<&str> = palavras_nome(nome).collect::<HashSet<_>>().into_iter().collect();
        palavras.sort_by_key(|p| self.por_palavra.get(*p).map_or(0, Vec::len));
        palavras
            .iter()
            .take(2)
            .flat_map(|p| self.por_palavra.get(*p).into_iter().flatten().copied())
            .collect()
    }

    pub fn info(&self) -> Info {
        Info {
            arquivo: self.caminho.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            total_linhas: self.registros.len(),
            colunas: self.colunas.clone(),
            coluna_cpf: self.colunas[self.coluna_cpf].clone(),
            coluna_nome: self.colunas[self.coluna_nome].clone(),
            linhas_sem_cpf_valido: self.registros.iter().filter(|r| r.cpf.is_none()).count(),
        }
    }
}
