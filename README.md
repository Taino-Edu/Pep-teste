# API PEP

API em Rust (Axum) para verificar se um CPF/nome consta na lista de **Pessoas Expostas Politicamente (PEP)** — por exemplo, o CSV publicado no Portal da Transparência.

A lista PEP traz o CPF mascarado (`***.456.789-**`), então a API compara os dígitos visíveis do CPF (mínimo de 6 em comum) **e** o nome por similaridade (sem acentos/pontuação, maiúsculas). Só são retornados os registros em que **CPF e nome batem**.

## Requisitos

- [Rust](https://rustup.rs/) (edition 2024)

## Como rodar

1. Coloque a planilha PEP (`.csv`, `.xlsx`, `.xlsm` ou `.xls`) na pasta `dados/` — a mais recente é carregada automaticamente.
2. Rode:

```bash
cargo run --release
```

A API sobe em `http://127.0.0.1:8000`.

### Variáveis de ambiente (opcionais)

| Variável      | Descrição                                              | Padrão                       |
|---------------|--------------------------------------------------------|------------------------------|
| `PLANILHA`    | Caminho da planilha a carregar                         | arquivo mais recente em `dados/` |
| `COLUNA_CPF`  | Nome da coluna de CPF                                  | detecta coluna com "CPF"     |
| `COLUNA_NOME` | Nome da coluna de nome                                 | detecta coluna com "NOME"    |
| `PORTA`       | Porta HTTP                                             | `8000`                       |

## Endpoints

### `GET /planilha`
Informações da planilha carregada (arquivo, total de linhas, colunas, colunas de CPF/nome usadas, linhas sem CPF válido).

### `POST /planilha`
Envia uma nova planilha PEP (multipart, campo `arquivo`). Ela é salva em `dados/` e passa a ser usada.

```bash
curl -F "arquivo=@202608_PEP.csv" http://127.0.0.1:8000/planilha
```

### `GET /consulta`
Consulta um CPF e/ou nome.

| Parâmetro             | Descrição                                   |
|-----------------------|---------------------------------------------|
| `cpf`                 | CPF (com ou sem pontuação)                  |
| `nome`                | Nome completo                               |
| `similaridade_minima` | 0 a 1, padrão `0.85`                        |

```bash
curl "http://127.0.0.1:8000/consulta?cpf=123.456.789-00&nome=Fulano%20de%20Tal"
```

```json
{
  "total_iguais": 1,
  "iguais": [
    { "cpf": "123.456.789-00", "nome": "Fulano de Tal", "cpf_pep": "***.456.789-**", "nome_pep": "FULANO DE TAL" }
  ]
}
```

### `POST /cruzar`
Cruza uma lista inteira (`.xlsx`/`.csv` com colunas de CPF e nome) com a planilha PEP. Aceita `similaridade_minima` na query string.

```bash
curl -F "arquivo=@clientes.xlsx" "http://127.0.0.1:8000/cruzar?similaridade_minima=0.9"
```

Retorna `total_consultados`, `total_iguais`, `linhas_com_erro` e a lista `iguais` (com o número da `linha` na planilha enviada).

## Estrutura

```
src/
  main.rs        # rotas e estado da API
  planilha.rs    # leitura Excel/CSV, normalização de CPF/nome e índices
  comparador.rs  # comparação de CPF (mascarado) e nome (similaridade)
dados/           # planilhas PEP (não versionadas)
```
