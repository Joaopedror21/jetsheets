//! Deterministic Portuguese filter commands. This crate only builds a typed
//! expression; the data engine owns execution and result statistics.

use jetsheets_core::{
    AggregateOperation, AggregateSpec, ColumnKind, Condition, FilterExpr, FilterValue, Operator,
    SortDirection, SortSpec, Table, parse_br_number,
};
use std::fmt;

/// A filter and the exact interpretation shown before applying it.
#[derive(Debug, Clone)]
pub struct ParsedFilter {
    pub expr: FilterExpr,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSort {
    pub spec: SortSpec,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct ParsedAggregate {
    pub spec: AggregateSpec,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub enum ParsedCommand {
    Filter(ParsedFilter),
    Sort(ParsedSort),
    Aggregate(ParsedAggregate),
}

impl ParsedCommand {
    pub fn explanation(&self) -> &str {
        match self {
            Self::Filter(parsed) => &parsed.explanation,
            Self::Sort(parsed) => &parsed.explanation,
            Self::Aggregate(parsed) => &parsed.explanation,
        }
    }
}

pub fn parse_command(command: &str, table: &Table) -> Result<ParsedCommand, ParseError> {
    parse_command_with_override(command, table, None)
}

pub fn parse_command_with_override(
    command: &str,
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<ParsedCommand, ParseError> {
    let tokens = tokenize(command);
    if tokens
        .first()
        .is_some_and(|token| is_sort_intent(&token.norm))
    {
        parse_sort(&tokens, table, chosen_column).map(ParsedCommand::Sort)
    } else if tokens
        .first()
        .is_some_and(|token| is_aggregate_intent(&token.norm))
    {
        parse_aggregate(&tokens, table, chosen_column).map(ParsedCommand::Aggregate)
    } else {
        parse_filter_with_override(command, table, chosen_column).map(ParsedCommand::Filter)
    }
}

fn is_aggregate_intent(word: &str) -> bool {
    matches!(
        word,
        "agrupe"
            | "agrupar"
            | "conte"
            | "contar"
            | "some"
            | "somar"
            | "media"
            | "calcule"
            | "calcular"
    )
}

fn parse_aggregate(
    tokens: &[Token],
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<ParsedAggregate, ParseError> {
    let by = tokens
        .iter()
        .position(|token| token.norm == "por")
        .ok_or_else(|| {
            ParseError::Unsupported("informe a coluna de agrupamento após 'por'".into())
        })?;
    let group_tokens = &tokens[by + 1..];
    if group_tokens.is_empty() {
        return Err(ParseError::Unsupported(
            "informe a coluna de agrupamento após 'por'".into(),
        ));
    }
    let group_column = resolve_column(group_tokens, table, chosen_column)?;
    let is_sum = matches!(tokens[0].norm.as_str(), "some" | "somar");
    let is_average = matches!(tokens[0].norm.as_str(), "media")
        || matches!(tokens[0].norm.as_str(), "calcule" | "calcular")
            && tokens[1..by].iter().any(|token| token.norm == "media");
    if matches!(tokens[0].norm.as_str(), "calcule" | "calcular") && !is_average {
        return Err(ParseError::Unsupported(
            "informe 'média' e a coluna numérica".into(),
        ));
    }
    let operation = if is_sum || is_average {
        let mut value_tokens = &tokens[1..by];
        if is_average {
            if let Some(position) = value_tokens.iter().position(|token| token.norm == "media") {
                value_tokens = &value_tokens[position + 1..];
            }
            while value_tokens
                .first()
                .is_some_and(|token| matches!(token.norm.as_str(), "de" | "do" | "da"))
            {
                value_tokens = &value_tokens[1..];
            }
        }
        if value_tokens.is_empty() {
            return Err(ParseError::Unsupported(
                "informe a coluna numérica antes de 'por'".into(),
            ));
        }
        let value_column = resolve_column(value_tokens, table, chosen_column)?;
        if !is_numeric_kind(&table.columns[value_column].kind) {
            return Err(ParseError::InvalidValue(format!(
                "'{}' não é uma coluna numérica",
                table.columns[value_column].name
            )));
        }
        if is_average {
            AggregateOperation::Average { value_column }
        } else {
            AggregateOperation::Sum { value_column }
        }
    } else {
        AggregateOperation::Count
    };
    let explanation = match operation {
        AggregateOperation::Count => format!(
            "Contar registros por '{}'.",
            table.columns[group_column].name
        ),
        AggregateOperation::Sum { value_column } => format!(
            "Somar '{}' por '{}'.",
            table.columns[value_column].name, table.columns[group_column].name
        ),
        AggregateOperation::Average { value_column } => format!(
            "Calcular média de '{}' por '{}'.",
            table.columns[value_column].name, table.columns[group_column].name
        ),
    };
    Ok(ParsedAggregate {
        spec: AggregateSpec {
            group_column,
            operation,
        },
        explanation,
    })
}

fn is_sort_intent(word: &str) -> bool {
    matches!(
        word,
        "ordene" | "ordenar" | "ordena" | "organize" | "classifique"
    )
}

fn parse_sort(
    tokens: &[Token],
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<ParsedSort, ParseError> {
    let explicit_ascending = tokens
        .iter()
        .any(|token| matches!(token.norm.as_str(), "crescente" | "ascendente"));
    let explicit_descending = tokens
        .iter()
        .any(|token| matches!(token.norm.as_str(), "decrescente" | "descendente"));
    let greater = tokens.iter().position(|token| token.norm == "maior");
    let lesser = tokens.iter().position(|token| token.norm == "menor");
    let word_direction = match (greater, lesser) {
        (Some(greater), Some(lesser)) if greater < lesser => Some(SortDirection::Descending),
        (Some(_), Some(_)) => Some(SortDirection::Ascending),
        (Some(_), None) => Some(SortDirection::Descending),
        (None, Some(_)) => Some(SortDirection::Ascending),
        (None, None) => None,
    };
    if explicit_ascending && explicit_descending
        || explicit_ascending && word_direction == Some(SortDirection::Descending)
        || explicit_descending && word_direction == Some(SortDirection::Ascending)
    {
        return Err(ParseError::InvalidValue(
            "a direção da ordenação é contraditória".into(),
        ));
    }
    let direction = if explicit_descending {
        SortDirection::Descending
    } else if explicit_ascending {
        SortDirection::Ascending
    } else {
        word_direction.unwrap_or(SortDirection::Ascending)
    };

    let prefix = if let Some(by) = tokens.iter().position(|token| token.norm == "por") {
        let after_by = &tokens[by + 1..];
        let end = after_by
            .iter()
            .position(|token| is_sort_direction_word(&token.norm))
            .unwrap_or(after_by.len());
        if after_by[end..].iter().any(|token| {
            !is_sort_direction_word(&token.norm)
                && !matches!(token.norm.as_str(), "do" | "da" | "de" | "para" | "o" | "a")
        }) {
            return Err(ParseError::Unsupported(
                "há palavras não reconhecidas após a direção da ordenação".into(),
            ));
        }
        &after_by[..end]
    } else if let Some(direction_at) = tokens
        .iter()
        .position(|token| is_sort_direction_word(&token.norm))
    {
        if tokens[direction_at + 1..].iter().any(|token| {
            !is_filler(&token.norm)
                && !is_sort_direction_word(&token.norm)
                && !matches!(token.norm.as_str(), "para" | "do" | "da")
        }) {
            tokens
        } else {
            &tokens[1..direction_at]
        }
    } else {
        &tokens[1..]
    };
    if prefix.is_empty() {
        return Err(ParseError::Unsupported(
            "informe a coluna para ordenar (ex.: 'ordene por Valor decrescente')".into(),
        ));
    }
    let column = resolve_column(prefix, table, chosen_column)?;
    let spec = SortSpec { column, direction };
    let order = match direction {
        SortDirection::Ascending => "crescente",
        SortDirection::Descending => "decrescente",
    };
    Ok(ParsedSort {
        spec,
        explanation: format!(
            "Ordenar pela coluna '{}' em ordem {order}.",
            table.columns[column].name
        ),
    })
}

fn is_sort_direction_word(word: &str) -> bool {
    matches!(
        word,
        "maior" | "menor" | "crescente" | "decrescente" | "ascendente" | "descendente"
    )
}

/// User-facing failure modes. An ambiguous column is never selected silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Unsupported(String),
    Ambiguous {
        term: String,
        candidates: Vec<String>,
    },
    UnknownColumn(String),
    InvalidValue(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(detail) => write!(f, "Comando não suportado nesta versão: {detail}"),
            Self::Ambiguous { term, candidates } => write!(
                f,
                "A coluna '{term}' é ambígua. Escolha: {}.",
                candidates.join(" ou ")
            ),
            Self::UnknownColumn(name) => write!(f, "Coluna não encontrada: '{name}'."),
            Self::InvalidValue(detail) => write!(f, "Valor ou condição inválida: {detail}"),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone)]
struct Token {
    raw: String,
    norm: String,
    quoted: bool,
}

/// Parse a filter without any UI decision. Ambiguous aliases return candidates.
pub fn parse_filter(command: &str, table: &Table) -> Result<ParsedFilter, ParseError> {
    parse_filter_with_override(command, table, None)
}

/// Resolve an ambiguous alias with a column selected by the user. The selected
/// index is accepted only if it is one of that alias's candidates.
pub fn parse_filter_with_override(
    command: &str,
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<ParsedFilter, ParseError> {
    let tokens = tokenize(command);
    if tokens.is_empty() {
        return Err(ParseError::Unsupported(
            "escreva uma condição de filtro".into(),
        ));
    }
    if is_other_intent(&tokens) {
        return Err(ParseError::Unsupported(
            "este tipo de operação ainda não está disponível; use 'Filtre ...' ou 'Mostre ...' com uma condição".into(),
        ));
    }

    let mut groups: Vec<Vec<FilterExpr>> = vec![Vec::new()];
    let mut pos = 0;
    while pos < tokens.len() {
        let (expr, next) = parse_condition(&tokens, pos, table, chosen_column)?;
        groups.last_mut().expect("initial group exists").push(expr);
        if next == tokens.len() {
            break;
        }
        match tokens[next].norm.as_str() {
            "e" => {}
            "ou" => groups.push(Vec::new()),
            _ => {
                return Err(ParseError::Unsupported(format!(
                    "trecho '{}' após a condição",
                    tokens[next].raw
                )));
            }
        }
        pos = next + 1;
        if pos == tokens.len() {
            return Err(ParseError::InvalidValue(
                "falta a condição após o conector".into(),
            ));
        }
    }
    let alternatives: Vec<FilterExpr> = groups
        .into_iter()
        .map(|mut conditions| {
            if conditions.len() == 1 {
                conditions.remove(0)
            } else {
                FilterExpr::And(conditions)
            }
        })
        .collect();
    let expr = if alternatives.len() == 1 {
        alternatives
            .into_iter()
            .next()
            .expect("one alternative exists")
    } else {
        FilterExpr::Or(alternatives)
    };
    let explanation = describe(&expr, table);
    Ok(ParsedFilter { expr, explanation })
}

fn parse_condition(
    tokens: &[Token],
    start: usize,
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<(FilterExpr, usize), ParseError> {
    let (operator_start, operator_end, operator) = (start..tokens.len())
        .find_map(|at| match_operator(tokens, at).map(|(end, op)| (at, end, op)))
        .ok_or_else(|| {
            ParseError::Unsupported(
                "nenhuma comparação reconhecida (ex.: 'valor acima de 5000')".into(),
            )
        })?;
    let mut prefix = &tokens[start..operator_start];
    let negated = prefix.iter().any(|token| token.norm == "nao");
    if prefix.last().is_some_and(|token| token.norm == "nao") {
        prefix = &prefix[..prefix.len() - 1];
    }
    let column = resolve_column(prefix, table, chosen_column)?;
    let kind = &table.columns[column].kind;
    let mut cursor = operator_end;
    let value = match operator {
        Operator::Greater
        | Operator::GreaterEqual
        | Operator::Less
        | Operator::LessEqual
        | Operator::Between => {
            if !is_numeric_kind(kind) {
                return Err(ParseError::InvalidValue(format!(
                    "'{}' não é uma coluna numérica",
                    table.columns[column].name
                )));
            }
            let (first, next) = read_number(tokens, cursor)?;
            cursor = next;
            if matches!(operator, Operator::Between) {
                while tokens
                    .get(cursor)
                    .is_some_and(|token| matches!(token.norm.as_str(), "reais" | "real"))
                {
                    cursor += 1;
                }
                if tokens.get(cursor).is_none_or(|token| token.norm != "e") {
                    return Err(ParseError::InvalidValue(
                        "'entre' exige dois números separados por 'e'".into(),
                    ));
                }
                let (second, next) = read_number(tokens, cursor + 1)?;
                if first > second {
                    return Err(ParseError::InvalidValue(
                        "o primeiro limite de 'entre' deve ser menor ou igual ao segundo".into(),
                    ));
                }
                cursor = next;
                FilterValue::Range(first, second)
            } else {
                FilterValue::Number(first)
            }
        }
        Operator::Contains | Operator::StartsWith | Operator::EndsWith => {
            if !matches!(kind, ColumnKind::Text) {
                return Err(ParseError::InvalidValue(format!(
                    "'{}' não é uma coluna de texto",
                    table.columns[column].name
                )));
            }
            let (text, next) = read_text(tokens, cursor)?;
            cursor = next;
            FilterValue::Text(text)
        }
        Operator::Equal | Operator::NotEqual => {
            if is_numeric_kind(kind) {
                let (number, next) = read_number(tokens, cursor)?;
                cursor = next;
                FilterValue::Number(number)
            } else if matches!(kind, ColumnKind::Text) {
                let (text, next) = read_text(tokens, cursor)?;
                cursor = next;
                FilterValue::Text(text)
            } else {
                return Err(ParseError::Unsupported(
                    "comparação de datas ainda não está disponível".into(),
                ));
            }
        }
    };
    while tokens
        .get(cursor)
        .is_some_and(|token| matches!(token.norm.as_str(), "reais" | "real" | "unidades"))
    {
        cursor += 1;
    }
    if let Some(token) = tokens.get(cursor)
        && token.norm != "e"
        && token.norm != "ou"
    {
        return Err(ParseError::Unsupported(format!(
            "trecho '{}' após o valor",
            token.raw
        )));
    }
    let condition = FilterExpr::Condition(Condition {
        column,
        operator,
        value,
    });
    Ok((
        if negated {
            FilterExpr::Not(Box::new(condition))
        } else {
            condition
        },
        cursor,
    ))
}

fn is_numeric_kind(kind: &ColumnKind) -> bool {
    matches!(
        kind,
        ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage
    )
}

fn read_number(tokens: &[Token], start: usize) -> Result<(f64, usize), ParseError> {
    let mut at = start;
    if tokens.get(at).is_some_and(|token| token.norm == "r$") {
        at += 1;
    }
    let token = tokens
        .get(at)
        .ok_or_else(|| ParseError::InvalidValue("falta um número".into()))?;
    let number = parse_br_number(&token.raw)
        .ok_or_else(|| ParseError::InvalidValue(format!("'{}' não é um número", token.raw)))?;
    at += 1;
    let number = if tokens.get(at).is_some_and(|token| token.norm == "mil") {
        at += 1;
        number * 1000.0
    } else {
        number
    };
    if !number.is_finite() {
        return Err(ParseError::InvalidValue(
            "número fora do intervalo permitido".into(),
        ));
    }
    Ok((number, at))
}

fn read_text(tokens: &[Token], start: usize) -> Result<(String, usize), ParseError> {
    let mut at = start;
    let beginning = at;
    while tokens
        .get(at)
        .is_some_and(|token| token.quoted || (token.norm != "e" && token.norm != "ou"))
    {
        at += 1;
    }
    if at == beginning {
        return Err(ParseError::InvalidValue(
            "falta o texto para comparação".into(),
        ));
    }
    Ok((
        tokens[beginning..at]
            .iter()
            .map(|token| token.raw.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        at,
    ))
}

fn match_operator(tokens: &[Token], at: usize) -> Option<(usize, Operator)> {
    // Longest phrases first, so the "ou" in "maior ou igual" is not a connector.
    const PATTERNS: &[(&[&str], Operator)] = &[
        (&["nao", "e", "igual", "a"], Operator::NotEqual),
        (&["maior", "ou", "igual", "a"], Operator::GreaterEqual),
        (&["maior", "ou", "igual", "que"], Operator::GreaterEqual),
        (&["menor", "ou", "igual", "a"], Operator::LessEqual),
        (&["menor", "ou", "igual", "que"], Operator::LessEqual),
        (&["maior", "ou", "igual"], Operator::GreaterEqual),
        (&["menor", "ou", "igual"], Operator::LessEqual),
        (&["maior", "do", "que"], Operator::Greater),
        (&["menor", "do", "que"], Operator::Less),
        (&["no", "minimo"], Operator::GreaterEqual),
        (&["pelo", "menos"], Operator::GreaterEqual),
        (&["no", "maximo"], Operator::LessEqual),
        (&["maior", "que"], Operator::Greater),
        (&["menor", "que"], Operator::Less),
        (&["mais", "de"], Operator::Greater),
        (&["menos", "de"], Operator::Less),
        (&["acima", "de"], Operator::Greater),
        (&["abaixo", "de"], Operator::Less),
        (&["superior", "a"], Operator::Greater),
        (&["inferior", "a"], Operator::Less),
        (&["igual", "a"], Operator::Equal),
        (&["e", "igual", "a"], Operator::Equal),
        (&["iguais", "a"], Operator::Equal),
        (&["diferente", "de"], Operator::NotEqual),
        (&["diferentes", "de"], Operator::NotEqual),
        (&["comeca", "com"], Operator::StartsWith),
        (&["termina", "com"], Operator::EndsWith),
        (&["inicia", "com"], Operator::StartsWith),
        (&["entre"], Operator::Between),
        (&["contem"], Operator::Contains),
        (&["exatamente"], Operator::Equal),
        (&["ate"], Operator::LessEqual),
        (&[">="], Operator::GreaterEqual),
        (&["<="], Operator::LessEqual),
        (&["!="], Operator::NotEqual),
        (&["<>"], Operator::NotEqual),
        (&[">"], Operator::Greater),
        (&["<"], Operator::Less),
        (&["="], Operator::Equal),
    ];
    PATTERNS.iter().find_map(|(phrase, op)| {
        tokens
            .get(at..at + phrase.len())
            .filter(|slice| {
                slice
                    .iter()
                    .zip(*phrase)
                    .all(|(token, word)| token.norm == *word)
            })
            .map(|_| (at + phrase.len(), *op))
    })
}

fn resolve_column(
    prefix: &[Token],
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<usize, ParseError> {
    // Exact column names take precedence over broad aliases.
    let mut exact: Vec<(usize, usize, usize)> = Vec::new();
    let last_meaningful = prefix
        .iter()
        .rposition(|token| !is_filler(&token.norm) && token.norm != "nao")
        .map(|index| index + 1);
    for (index, column) in table.columns.iter().enumerate() {
        let name: Vec<String> = normalize(&column.name)
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        if name.is_empty() {
            continue;
        }
        for end in name.len()..=prefix.len() {
            if Some(end) != last_meaningful {
                continue;
            }
            if prefix[end - name.len()..end]
                .iter()
                .map(|token| token.norm.as_str())
                .eq(name.iter().map(String::as_str))
            {
                exact.push((index, name.len(), end));
            }
        }
    }
    if let Some(best_length) = exact.iter().map(|(_, length, _)| *length).max() {
        let best_end = exact
            .iter()
            .filter(|(_, length, _)| *length == best_length)
            .map(|(_, _, end)| *end)
            .max()
            .expect("an exact match exists");
        let candidates: Vec<usize> = exact
            .iter()
            .filter(|(_, length, end)| *length == best_length && *end == best_end)
            .map(|(index, _, _)| *index)
            .collect();
        let term = table.columns[candidates[0]].name.clone();
        return select_column(&term, candidates, table, chosen_column);
    }

    let term = prefix
        .iter()
        .rev()
        .find(|token| !is_filler(&token.norm) && token.norm != "nao")
        .map(|token| token.norm.as_str())
        .unwrap_or("");
    let class = alias_class(term);
    let candidates: Vec<usize> = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| {
            let normalized = normalize(&column.name);
            normalized.split_whitespace().any(|part| part == term)
                || class.is_some_and(|class| {
                    normalized
                        .split_whitespace()
                        .any(|part| alias_class(part) == Some(class))
                })
        })
        .map(|(index, _)| index)
        .collect();
    if candidates.is_empty() {
        let raw = prefix
            .iter()
            .rev()
            .find(|token| !is_filler(&token.norm) && token.norm != "nao")
            .map(|token| token.raw.clone())
            .unwrap_or_else(|| "(não informada)".into());
        return Err(ParseError::UnknownColumn(raw));
    }
    select_column(term, candidates, table, chosen_column)
}

fn select_column(
    term: &str,
    candidates: Vec<usize>,
    table: &Table,
    chosen_column: Option<usize>,
) -> Result<usize, ParseError> {
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    if let Some(index) = chosen_column
        && candidates.contains(&index)
    {
        return Ok(index);
    }
    Err(ParseError::Ambiguous {
        term: term.into(),
        candidates: candidates
            .into_iter()
            .map(|index| table.columns[index].name.clone())
            .collect(),
    })
}

fn alias_class(term: &str) -> Option<&'static str> {
    match term {
        "valor" | "preco" | "custam" | "custa" | "custo" => Some("price"),
        "quantidade" | "qtd" | "estoque" | "unidades" => Some("quantity"),
        "produto" | "produtos" | "item" | "itens" | "descricao" => Some("product"),
        _ => None,
    }
}

fn is_filler(word: &str) -> bool {
    matches!(
        word,
        "filtre"
            | "filtrar"
            | "mostre"
            | "mostrar"
            | "me"
            | "os"
            | "as"
            | "o"
            | "a"
            | "um"
            | "uma"
            | "que"
            | "com"
            | "tem"
            | "tenham"
            | "de"
            | "do"
            | "da"
    )
}

fn is_other_intent(tokens: &[Token]) -> bool {
    tokens.iter().take(1).any(|token| {
        matches!(
            token.norm.as_str(),
            "ordene"
                | "ordenar"
                | "ordena"
                | "agrupe"
                | "agrupar"
                | "compare"
                | "comparar"
                | "grafico"
                | "graficos"
                | "some"
                | "somar"
                | "soma"
                | "media"
                | "mediana"
                | "conte"
                | "contar"
        )
    })
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let push = |current: &mut String, tokens: &mut Vec<Token>, quoted: bool| {
        if !current.is_empty() {
            let raw = std::mem::take(current);
            let norm = normalize(&raw);
            if !norm.is_empty() {
                tokens.push(Token { raw, norm, quoted });
            }
        }
    };
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(quote_char) = quote {
            if ch == quote_char {
                push(&mut current, &mut tokens, true);
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            push(&mut current, &mut tokens, false);
            quote = Some(ch);
        } else if matches!(ch, '>' | '<' | '=' | '!') {
            push(&mut current, &mut tokens, false);
            let mut symbol = ch.to_string();
            if chars
                .peek()
                .is_some_and(|next| *next == '=' || (ch == '<' && *next == '>'))
            {
                symbol.push(chars.next().expect("peeked symbol exists"));
            }
            if symbol != "!" {
                let norm = symbol.clone();
                tokens.push(Token {
                    raw: symbol,
                    norm,
                    quoted: false,
                });
            }
        } else if ch.is_whitespace()
            || matches!(ch, ';' | ':' | '?' | '(' | ')')
            || (matches!(ch, ',' | '.')
                && !(current
                    .chars()
                    .last()
                    .is_some_and(|previous| previous.is_ascii_digit())
                    && chars.peek().is_some_and(char::is_ascii_digit)))
        {
            push(&mut current, &mut tokens, false);
        } else {
            current.push(ch);
        }
    }
    push(&mut current, &mut tokens, quote.is_some());
    tokens
}

fn normalize(input: &str) -> String {
    input
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect::<String>()
        .trim_matches(|ch: char| ch == '.' || ch == ',')
        .to_owned()
}

fn describe(expr: &FilterExpr, table: &Table) -> String {
    match expr {
        FilterExpr::Condition(condition) => {
            let column = &table.columns[condition.column].name;
            let relation = match condition.operator {
                Operator::Greater => "maior que",
                Operator::GreaterEqual => "maior ou igual a",
                Operator::Less => "menor que",
                Operator::LessEqual => "menor ou igual a",
                Operator::Equal => "igual a",
                Operator::NotEqual => "diferente de",
                Operator::Between => "entre",
                Operator::Contains => "contém",
                Operator::StartsWith => "começa com",
                Operator::EndsWith => "termina com",
            };
            let value = match &condition.value {
                FilterValue::Number(number) => format_number(*number),
                FilterValue::Text(text) => format!("'{text}'"),
                FilterValue::Range(start, end) => {
                    format!("{} e {}", format_number(*start), format_number(*end))
                }
            };
            format!("{column} {relation} {value}")
        }
        FilterExpr::And(parts) => parts
            .iter()
            .map(|part| describe(part, table))
            .collect::<Vec<_>>()
            .join(" E "),
        FilterExpr::Or(parts) => parts
            .iter()
            .map(|part| describe(part, table))
            .collect::<Vec<_>>()
            .join(" OU "),
        FilterExpr::Not(inner) => format!("NÃO ({})", describe(inner, table)),
    }
}

fn format_number(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        number.to_string().replace('.', ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetsheets_core::{evaluate_filter, sort_rows};

    fn products() -> Table {
        let mut table = Table::new(
            "Produtos",
            vec![
                "Produto".into(),
                "Valor".into(),
                "Quantidade".into(),
                "Categoria".into(),
            ],
        );
        for row in [
            ["A", "6.000", "5.000", "Eletrônicos"],
            ["B", "5.000", "4.500", "Livros"],
            ["C", "8.000", "3.000", "Eletrônicos"],
            ["D", "", "6.000", "Livros"],
        ] {
            table.add_row(row.map(String::from).to_vec()).unwrap();
        }
        table.infer_column_types();
        table
    }

    #[test]
    fn parses_acceptance_command_and_evaluates_only_matching_rows() {
        let table = products();
        let parsed = parse_filter(
            "Filtre os produtos com valor acima de 5000 e quantidade maior que 4000",
            &table,
        )
        .unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0]);
        assert_eq!(
            parsed.explanation,
            "Valor maior que 5000 E Quantidade maior que 4000"
        );
    }

    #[test]
    fn understands_portuguese_sort_and_requires_unambiguous_column() {
        let table = products();
        for command in [
            "Ordene do maior para o menor valor",
            "Ordene por valor decrescente",
        ] {
            let ParsedCommand::Sort(parsed) = parse_command(command, &table).unwrap() else {
                panic!("esperada ordenação");
            };
            assert_eq!(parsed.spec.column, 1);
            assert_eq!(parsed.spec.direction, SortDirection::Descending);
            assert_eq!(
                sort_rows(&table, &[0, 1, 2, 3], &parsed.spec).unwrap(),
                vec![2, 0, 1, 3]
            );
        }
        let ParsedCommand::Sort(parsed) =
            parse_command("Ordene por valor crescente", &table).unwrap()
        else {
            panic!("esperada ordenação");
        };
        assert_eq!(parsed.spec.direction, SortDirection::Ascending);
        assert!(parse_command("Ordene por valor crescente decrescente", &table).is_err());
        assert!(parse_command("Ordene por valor crescente e apague os dados", &table).is_err());

        let mut ambiguous = Table::new(
            "Teste",
            vec!["Valor de compra".into(), "Valor de venda".into()],
        );
        ambiguous.add_row(vec!["1".into(), "2".into()]).unwrap();
        ambiguous.infer_column_types();
        assert!(matches!(
            parse_command("Ordene por valor", &ambiguous),
            Err(ParseError::Ambiguous { .. })
        ));
        let parsed = parse_command_with_override("Ordene por valor", &ambiguous, Some(1)).unwrap();
        assert!(matches!(
            parsed,
            ParsedCommand::Sort(ParsedSort {
                spec: SortSpec { column: 1, .. },
                ..
            })
        ));
    }

    #[test]
    fn parses_group_counts_and_sums_without_guessing_columns() {
        let table = products();
        let ParsedCommand::Aggregate(count) =
            parse_command("Agrupe os produtos por categoria", &table).unwrap()
        else {
            panic!("esperado agrupamento");
        };
        assert_eq!(count.spec.group_column, 3);
        assert_eq!(count.spec.operation, AggregateOperation::Count);
        let ParsedCommand::Aggregate(sum) =
            parse_command("Some valor por categoria", &table).unwrap()
        else {
            panic!("esperada soma");
        };
        assert_eq!(
            sum.spec.operation,
            AggregateOperation::Sum { value_column: 1 }
        );
        assert_eq!(sum.spec.group_column, 3);
        let ParsedCommand::Aggregate(average) =
            parse_command("Calcule a média de valor por categoria", &table).unwrap()
        else {
            panic!("esperada média");
        };
        assert_eq!(
            average.spec.operation,
            AggregateOperation::Average { value_column: 1 }
        );
        assert!(parse_command("Calcule valor por categoria", &table).is_err());
        assert!(parse_command("Some categoria por produto", &table).is_err());
        assert!(parse_command("Some valor por", &table).is_err());
    }

    #[test]
    fn and_has_precedence_over_or() {
        let table = products();
        let parsed = parse_filter(
            "valor acima de 7000 ou quantidade acima de 4000 e valor abaixo de 6000",
            &table,
        )
        .unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![1, 2]);
    }

    #[test]
    fn money_ranges_and_inclusive_boundaries_work() {
        let table = products();
        let parsed = parse_filter(
            "Mostre os produtos que custam entre R$ 5.000 e 6 mil reais",
            &table,
        )
        .unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0, 1]);
    }

    #[test]
    fn subject_name_does_not_hide_later_column_alias() {
        let mut table = Table::new("Vendas", vec!["Produtos".into(), "Preço".into()]);
        table.add_row(vec!["A".into(), "120".into()]).unwrap();
        table.infer_column_types();
        let parsed = parse_filter(
            "Mostre os produtos que custam entre 100 e 500 reais",
            &table,
        )
        .unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0]);
    }

    #[test]
    fn symbolic_operators_and_sentence_punctuation_work() {
        let table = products();
        let parsed = parse_filter("Valor>=5.000,50.", &table).unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0, 2]);
    }

    #[test]
    fn text_search_and_negation_work() {
        let table = products();
        let parsed = parse_filter("categoria não contém Eletrônicos", &table).unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![1, 3]);
    }

    #[test]
    fn quoted_connector_is_a_text_value() {
        let mut table = Table::new("Palavras", vec!["Palavra".into()]);
        table.add_row(vec!["e".into()]).unwrap();
        table.add_row(vec!["a".into()]).unwrap();
        let conjunction = parse_filter("palavra igual a 'e'", &table).unwrap();
        let article = parse_filter("palavra igual a 'a'", &table).unwrap();
        assert_eq!(evaluate_filter(&table, &conjunction.expr).unwrap(), vec![0]);
        assert_eq!(evaluate_filter(&table, &article.expr).unwrap(), vec![1]);
    }

    #[test]
    fn portuguese_not_equal_phrase_is_recognized() {
        let table = products();
        let parsed = parse_filter("categoria não é igual a Livros", &table).unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0, 2]);
    }

    #[test]
    fn ambiguous_alias_requires_selection() {
        let mut table = Table::new(
            "Estoque",
            vec!["Quantidade em estoque".into(), "Quantidade vendida".into()],
        );
        table.add_row(vec!["8".into(), "3".into()]).unwrap();
        table.infer_column_types();
        let command = "Filtre quantidade acima de 5";
        assert!(matches!(
            parse_filter(command, &table),
            Err(ParseError::Ambiguous { candidates, .. }) if candidates.len() == 2
        ));
        let parsed = parse_filter_with_override(command, &table, Some(0)).unwrap();
        assert_eq!(evaluate_filter(&table, &parsed.expr).unwrap(), vec![0]);
        assert!(matches!(
            parse_filter_with_override(command, &table, Some(9)),
            Err(ParseError::Ambiguous { .. })
        ));
    }

    #[test]
    fn invalid_columns_and_future_operations_are_explicit() {
        let table = products();
        assert!(matches!(
            parse_filter("saldo acima de 5", &table),
            Err(ParseError::UnknownColumn(_))
        ));
        assert!(matches!(
            parse_filter("Ordene do maior para o menor valor", &table),
            Err(ParseError::Unsupported(_))
        ));
        assert!(matches!(
            parse_filter("Valor acima de qualquer coisa", &table),
            Err(ParseError::InvalidValue(_))
        ));
    }
}
