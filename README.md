# API PEP

API em Rust (Axum) para verificar se um CPF/nome consta na lista de **Pessoas Expostas Politicamente (PEP)** publicada no [Portal da Transparência](https://portaldatransparencia.gov.br/download-de-dados/pep).

A lista PEP traz o CPF mascarado (`***.456.789-**`), então a API compara:

- **CPF**: os dígitos visíveis precisam ser todos iguais (mínimo de 6 em comum);
- **Nome**: sem acentos/pontuação e em maiúsculas, igual ou com similaridade (Levenshtein) acima do mínimo — padrão `0.85`.

Só são retornados os registros em que **CPF e nome batem**.

## Requisitos

- [Rust](https://rustup.rs/) (edition 2024)

## Como rodar

```bash
cargo run --release
```

A API sobe em `http://127.0.0.1:8000` e já carrega a lista PEP que vem no repositório (`dados/202608_PEP.csv`, agosto/2026).

Para usar outra lista, coloque o arquivo (`.csv`, `.xlsx`, `.xlsm` ou `.xls`) em `dados/` — o mais recente é carregado ao iniciar — ou envie por `POST /planilha`.

### Variáveis de ambiente (opcionais)

| Variável      | Descrição                       | Padrão                            |
|---------------|---------------------------------|-----------------------------------|
| `PLANILHA`    | Caminho da planilha a carregar  | arquivo mais recente em `dados/`  |
| `COLUNA_CPF`  | Nome da coluna de CPF           | detecta coluna com "CPF"          |
| `COLUNA_NOME` | Nome da coluna de nome          | detecta coluna com "NOME"         |
| `PORTA`       | Porta HTTP                      | `8000`                            |

## Endpoints

### `GET /planilha`
Informações da planilha carregada: arquivo, total de linhas, colunas, colunas de CPF/nome usadas e linhas sem CPF válido.

### `POST /planilha`
Envia uma nova lista PEP (multipart, campo `arquivo`). Ela é salva em `dados/` e passa a ser usada.

```bash
curl -F "arquivo=@202609_PEP.csv" http://127.0.0.1:8000/planilha
```

### `GET /consulta`
Consulta uma pessoa. Informe **CPF e nome** — como o resultado exige os dois batendo, mandar só um deles sempre retorna `total_iguais: 0`.

| Parâmetro             | Descrição                         |
|-----------------------|-----------------------------------|
| `cpf`                 | CPF (com ou sem pontuação)        |
| `nome`                | Nome completo                     |
| `similaridade_minima` | 0 a 1, padrão `0.85`              |

```bash
curl "http://127.0.0.1:8000/consulta?cpf=123.456.789-00&nome=Fulano%20de%20Tal"
```

```json
{
  "total_iguais": 1,
  "iguais": [
    {
      "cpf": "123.456.789-00",
      "nome": "FULANO DE TAL",
      "cpf_pep": "***.456.789-**",
      "nome_pep": "FULANO DE TAL"
    }
  ]
}
```

`cpf` e `nome` são os dados consultados (normalizados); `cpf_pep` e `nome_pep` são como aparecem na lista PEP.

### `POST /cruzar`
Cruza uma lista inteira (`.csv`, `.xlsx`, `.xlsm` ou `.xls` com colunas de CPF e nome) com a lista PEP. Aceita `similaridade_minima` na query string.

```bash
curl -F "arquivo=@clientes.xlsx" "http://127.0.0.1:8000/cruzar?similaridade_minima=0.9"
```

Retorna `total_consultados`, `total_iguais`, `linhas_com_erro` (ex.: CPF inválido) e a lista `iguais`, com a `linha` correspondente no arquivo enviado.

## Estrutura

```
src/
  main.rs        # rotas e estado da API
  planilha.rs    # leitura Excel/CSV, normalização de CPF/nome e índices de busca
  comparador.rs  # comparação de CPF mascarado e nome por similaridade
dados/
  202608_PEP.csv # lista PEP do Portal da Transparência (agosto/2026)
```
