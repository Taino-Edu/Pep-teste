//! Compara um CPF/nome consultado com as linhas da planilha.
use serde::Serialize;
use serde_json::{Map, Value};

use crate::planilha::{formatar_cpf, normalizar_cpf, normalizar_nome, Planilha};

/// Mínimo de dígitos visíveis em comum para considerar o CPF compatível
/// (a lista PEP mostra 6 dígitos: ***.456.789-**)
const MIN_DIGITOS_CPF: usize = 6;

#[derive(Serialize)]
pub struct ComparacaoCpf {
    pub compativel: bool,
    pub digitos_comparados: usize,
    pub digitos_iguais: usize,
    pub digitos_em_comum: String,
}

#[derive(Serialize)]
pub struct ComparacaoNome {
    pub igual: bool,
    pub similaridade: f64,
    pub palavras_iguais: Vec<String>,
    pub palavras_diferentes: Vec<String>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Resultado {
    CpfENome,
    SomenteCpf,
    SomenteNome,
}

#[derive(Serialize)]
pub struct Encontrado {
    pub resultado: Resultado,
    pub linha_planilha: usize,
    pub cpf_planilha: Option<String>,
    pub nome_planilha: Option<String>,
    pub comparacao_cpf: Option<ComparacaoCpf>,
    pub comparacao_nome: Option<ComparacaoNome>,
    pub dados: Map<String, Value>,
}

#[derive(Serialize, Default)]
pub struct Resumo {
    pub total_encontrados: usize,
    pub cpf_e_nome: usize,
    pub somente_cpf: usize,
    pub somente_nome: usize,
}

#[derive(Serialize)]
pub struct Consultado {
    pub cpf: Option<String>,
    pub nome: Option<String>,
}

#[derive(Serialize)]
pub struct RespostaConsulta {
    pub consulta: Consultado,
    pub resumo: Resumo,
    pub conclusao: String,
    pub encontrados: Vec<Encontrado>,
}

pub fn comparar_cpf(consulta: &str, cpf_planilha: &str) -> ComparacaoCpf {
    let (a, b) = (consulta.as_bytes(), cpf_planilha.as_bytes());
    let comparadas: Vec<usize> = (0..11).filter(|&p| a[p] != b'*' && b[p] != b'*').collect();
    let iguais: Vec<usize> = comparadas.iter().copied().filter(|&p| a[p] == b[p]).collect();
    let em_comum: String = (0..11).map(|p| if iguais.contains(&p) { b[p] as char } else { '*' }).collect();
    ComparacaoCpf {
        compativel: comparadas.len() >= MIN_DIGITOS_CPF && iguais.len() == comparadas.len(),
        digitos_comparados: comparadas.len(),
        digitos_iguais: iguais.len(),
        digitos_em_comum: formatar_cpf(&em_comum),
    }
}

pub fn comparar_nome(consulta: &str, nome_planilha: &str) -> ComparacaoNome {
    let a: Vec<&str> = consulta.split_whitespace().collect();
    let b: Vec<&str> = nome_planilha.split_whitespace().collect();
    let mut diferentes: Vec<String> = a
        .iter()
        .filter(|p| !b.contains(p))
        .chain(b.iter().filter(|p| !a.contains(p)))
        .map(|p| p.to_string())
        .collect();
    diferentes.sort();
    diferentes.dedup();
    ComparacaoNome {
        igual: consulta == nome_planilha,
        similaridade: (strsim::normalized_levenshtein(consulta, nome_planilha) * 100.0).round() / 100.0,
        palavras_iguais: a.iter().filter(|p| b.contains(p)).map(|p| p.to_string()).collect(),
        palavras_diferentes: diferentes,
    }
}

pub fn consultar(
    planilha: &Planilha,
    cpf: Option<&str>,
    nome: Option<&str>,
    similaridade_minima: f64,
) -> Result<RespostaConsulta, String> {
    let cpf = cpf.map(str::trim).filter(|s| !s.is_empty());
    let nome = nome.map(normalizar_nome).filter(|s| !s.is_empty());
    if cpf.is_none() && nome.is_none() {
        return Err("Informe o CPF e/ou o nome.".into());
    }
    let cpf = match cpf {
        Some(c) => Some(normalizar_cpf(c).ok_or_else(|| format!("CPF inválido: {c}"))?),
        None => None,
    };

    let mut candidatos = cpf.as_deref().map(|c| planilha.candidatos_cpf(c)).unwrap_or_default();
    if let Some(n) = &nome {
        candidatos.extend(planilha.candidatos_nome(n));
    }

    let mut encontrados: Vec<Encontrado> = candidatos
        .into_iter()
        .filter_map(|i| {
            let r = &planilha.registros[i];
            let c = cpf.as_deref().zip(r.cpf.as_deref()).map(|(a, b)| comparar_cpf(a, b));
            let n = nome.as_deref().map(|a| comparar_nome(a, &r.nome));
            let cpf_ok = c.as_ref().is_some_and(|c| c.compativel);
            let nome_ok = n.as_ref().is_some_and(|n| n.igual || n.similaridade >= similaridade_minima);
            let resultado = match (cpf_ok, nome_ok) {
                (true, true) => Resultado::CpfENome,
                (true, false) => Resultado::SomenteCpf,
                (false, true) => Resultado::SomenteNome,
                (false, false) => return None,
            };
            Some(Encontrado {
                resultado,
                linha_planilha: r.linha,
                cpf_planilha: r.cpf_original.clone(),
                nome_planilha: r.nome_original.clone(),
                comparacao_cpf: c,
                comparacao_nome: n,
                dados: r.dados.clone(),
            })
        })
        .collect();

    encontrados.sort_by(|a, b| {
        let sim = |e: &Encontrado| e.comparacao_nome.as_ref().map_or(0.0, |n| n.similaridade);
        a.resultado
            .cmp(&b.resultado)
            .then(sim(b).total_cmp(&sim(a)))
            .then(a.linha_planilha.cmp(&b.linha_planilha))
    });

    let conta = |r| encontrados.iter().filter(|e| e.resultado == r).count();
    let resumo = Resumo {
        total_encontrados: encontrados.len(),
        cpf_e_nome: conta(Resultado::CpfENome),
        somente_cpf: conta(Resultado::SomenteCpf),
        somente_nome: conta(Resultado::SomenteNome),
    };
    Ok(RespostaConsulta {
        consulta: Consultado { cpf: cpf.as_deref().map(formatar_cpf), nome },
        conclusao: conclusao(&resumo),
        resumo,
        encontrados,
    })
}

fn conclusao(r: &Resumo) -> String {
    if r.cpf_e_nome > 0 {
        format!("{} registro(s) com CPF e nome iguais.", r.cpf_e_nome)
    } else if r.somente_cpf > 0 && r.somente_nome > 0 {
        "CPF e nome aparecem, mas em registros diferentes. Conferir manualmente.".into()
    } else if r.somente_cpf > 0 {
        "Os dígitos do CPF batem, mas o nome é diferente.".into()
    } else if r.somente_nome > 0 {
        "O nome aparece, mas o CPF é diferente ou não foi informado.".into()
    } else {
        "Nenhum registro encontrado.".into()
    }
}
