//! Preset rules that work on a file's fields together (#850, #851, #852).
//!
//! The text rules of a preset rewrite one value from itself. These read and
//! write across a file's fields: set a field from a mask, remove or keep a list
//! of fields, read fields out of another one, split, merge or de-duplicate the
//! values of a multi-value field. They run in the same ordered pass as the text
//! rules, each seeing what the steps before it left, and their result is an
//! ordinary change: the plan gate, locks, Apply and Undo treat it like any other.
use super::*;

/// The rule kinds handled here rather than by a text transform chain.
const FIELD_RULE_KINDS: [&str; 7] = [
    "format",
    "remove_fields",
    "keep_fields",
    "extract",
    "split_values",
    "merge_values",
    "dedupe_values",
];

pub(crate) fn is_field_rule(kind: &str) -> bool {
    FIELD_RULE_KINDS.contains(&kind)
}

/// Why a field rule cannot run, said before any file is read.
#[derive(Debug, Error)]
pub enum FieldRuleError {
    #[error("a rule that sets a field needs one tag field to set")]
    NeedsField,
    #[error("{0} holds one value; splitting, merging and removing repeats work on artist, album artist, genre and composer")]
    NotMultiValue(String),
    #[error("an extraction mask has to name at least one field")]
    NothingToExtract,
    #[error("splitting values needs something to split on")]
    EmptyDelimiter,
    #[error("the joiner contains the multi-value separator, so the values would split apart again on write")]
    JoinerSplits,
}

/// One step of a preset run: a run of text rules over one scope, or a field rule.
pub(crate) enum Step {
    Chain(String, TransformChain),
    Field(FieldStep),
}

pub(crate) enum FieldStep {
    /// Set a field to what a mask renders.
    Format(TagField, Mask),
    /// Remove the listed fields, or with `keep` every text field not listed.
    Select {
        fields: Vec<TagField>,
        keep: bool,
    },
    /// Render the source mask and read fields out of it with the pattern.
    Extract(Mask, Mask),
    Split(TagField, String),
    Merge(TagField, String),
    Dedupe(TagField),
}

pub(crate) fn has_field_steps(steps: &[Step]) -> bool {
    steps.iter().any(|step| matches!(step, Step::Field(_)))
}

/// A group as the steps it runs as. Consecutive text rules on one scope are one
/// chain, as [`build_segments`] has always made them; a field rule stands alone
/// and keeps its place in the order. A disabled rule contributes nothing.
pub(crate) fn build_steps(group: &ActionGroupDto) -> Result<Vec<Step>, AppError> {
    let mut steps = Vec::new();
    let mut run: Option<(String, Vec<TransformRuleDto>)> = None;
    let flush = |run: &mut Option<(String, Vec<TransformRuleDto>)>,
                 steps: &mut Vec<Step>|
     -> Result<(), AppError> {
        if let Some((scope, rules)) = run.take() {
            steps.push(Step::Chain(scope, build_chain(&rules)?));
        }
        Ok(())
    };
    for rule in &group.rules {
        let scope = rule.scope.clone().unwrap_or_else(|| group.scope.clone());
        if is_field_rule(&rule.kind) {
            flush(&mut run, &mut steps)?;
            if rule.enabled {
                steps.push(Step::Field(compile(rule, &scope)?));
            }
            continue;
        }
        match &mut run {
            Some((current, rules)) if *current == scope => rules.push(rule.clone()),
            _ => {
                flush(&mut run, &mut steps)?;
                run = Some((scope, vec![rule.clone()]));
            }
        }
    }
    flush(&mut run, &mut steps)?;
    Ok(steps)
}

/// Every group's steps, in order, as one list.
pub(crate) fn build_all_steps(groups: &[ActionGroupDto]) -> Result<Vec<Step>, AppError> {
    let mut steps = Vec::new();
    for group in groups {
        steps.extend(build_steps(group)?);
    }
    Ok(steps)
}

fn compile(rule: &TransformRuleDto, scope: &str) -> Result<FieldStep, AppError> {
    let list =
        |text: &str| -> Vec<TagField> { text.split(';').filter_map(field_from_name).collect() };
    let multi = |scope: &str| -> Result<TagField, AppError> {
        let field = target_field(scope)?;
        if !field.is_multi_value() {
            return Err(FieldRuleError::NotMultiValue(field.to_storage_key()).into());
        }
        Ok(field)
    };
    Ok(match rule.kind.as_str() {
        "format" => {
            let field = target_field(scope)?;
            let mask = Mask::parse(&rule.to)?;
            if mask.is_extract_only() {
                return Err(MaskError::ExtractOnly.into());
            }
            FieldStep::Format(field, mask)
        }
        "remove_fields" => FieldStep::Select {
            fields: list(&rule.from),
            keep: false,
        },
        "keep_fields" => FieldStep::Select {
            fields: list(&rule.from),
            keep: true,
        },
        "extract" => {
            let source = Mask::parse(&rule.from)?;
            if source.is_extract_only() {
                return Err(MaskError::ExtractOnly.into());
            }
            let pattern = Mask::parse(&rule.to)?;
            pattern.check_extractable()?;
            if pattern.fields().is_empty() {
                return Err(FieldRuleError::NothingToExtract.into());
            }
            FieldStep::Extract(source, pattern)
        }
        "split_values" => {
            if rule.from.is_empty() {
                return Err(FieldRuleError::EmptyDelimiter.into());
            }
            FieldStep::Split(multi(scope)?, rule.from.clone())
        }
        "merge_values" => {
            let separator = tagrex_core::model::multi_value_separator();
            let core = separator.trim();
            let splits = if core.is_empty() {
                rule.to.contains(separator.as_str())
            } else {
                rule.to.contains(core)
            };
            if splits {
                return Err(FieldRuleError::JoinerSplits.into());
            }
            FieldStep::Merge(multi(scope)?, rule.to.clone())
        }
        "dedupe_values" => FieldStep::Dedupe(multi(scope)?),
        other => return Err(AppError::UnknownTransform(other.to_string())),
    })
}

/// The one field a rule writes to, named by its target.
fn target_field(scope: &str) -> Result<TagField, AppError> {
    if matches!(scope.trim(), "" | "tags" | "filename" | "fileext") {
        return Err(FieldRuleError::NeedsField.into());
    }
    field_from_name(scope).ok_or_else(|| FieldRuleError::NeedsField.into())
}

/// A field from what a person types or a preset stores: a storage key, a name
/// as the app shows it (`Album Artist`), a common tag spelling
/// (`TRACKNUMBER`, `LABEL`), or `custom:NAME`. Anything else is a custom
/// field of that name. Blank is nothing.
pub(crate) fn field_from_name(name: &str) -> Option<TagField> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(rest) = name
        .get(..7)
        .filter(|head| head.eq_ignore_ascii_case("custom:"))
        .map(|_| &name[7..])
    {
        let rest = rest.trim();
        return (!rest.is_empty()).then(|| TagField::Custom(rest.to_string()));
    }
    let key = loose_name(name);
    let standard = match key.as_str() {
        "tracknumber" | "trackno" => "track",
        "totaltracks" | "trackcount" => "tracktotal",
        "discnumber" | "discno" => "disc",
        "totaldiscs" | "disccount" => "disctotal",
        "initialkey" => "key",
        "label" | "organization" | "organisation" => "publisher",
        "catalog" | "catalogno" | "catno" => "catalognumber",
        "date" => "year",
        "mediatype" => "media",
        other => other,
    };
    match TagField::from_storage_key(standard) {
        TagField::Custom(_) => Some(TagField::Custom(name.to_string())),
        field => Some(field),
    }
}

/// A name with case, spaces, `_`, `-` and `#` taken out, so `Album Artist`,
/// `album_artist` and `ALBUMARTIST` are one name.
fn loose_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-' | '#'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether a field a rule names is this field of the file. A custom field
/// matches whatever spelling the file stores it under.
fn same_field(named: &TagField, stored: &TagField) -> bool {
    match (named, stored) {
        (TagField::Custom(a), TagField::Custom(b)) => loose_name(a) == loose_name(b),
        _ => named == stored,
    }
}

/// What one file's run collects besides its values.
#[derive(Default)]
pub(crate) struct FieldRun {
    /// A value a rule meant for a locked field: not applied, so no later step
    /// reads it, but handed to the plan gate so the lock is reported.
    pub blocked: Vec<(TagField, Option<String>)>,
    /// Files an extraction mask did not fit.
    pub not_extracted: usize,
}

impl FieldRun {
    /// `changes` with the blocked attempts added, for the gate to drop and
    /// report. One the changes already hold for that field is left as it is.
    pub fn add_blocked(&self, changes: &mut Vec<FieldChangeDto>, disk: &TagMap) {
        for (field, new) in &self.blocked {
            let key = field.to_storage_key();
            if changes.iter().any(|change| change.field == key) {
                continue;
            }
            let old = disk.get(field).cloned();
            if old != *new {
                changes.push(FieldChangeDto::new(key, old, new.clone()));
            }
        }
    }
}

impl App {
    /// Run one field rule over a file's values. `touched`, when given, collects
    /// the fields the rule changed: over a staged plan only those, and the ones
    /// the plan already proposes, become changes.
    pub(crate) fn run_field_step(
        &self,
        step: &FieldStep,
        track: &tagrex_core::model::TrackFile,
        tags: &mut TagMap,
        mut touched: Option<&mut Vec<TagField>>,
        run: &mut FieldRun,
    ) -> Result<(), AppError> {
        let locked = self.locked_fields.borrow();
        let mut assign = |tags: &mut TagMap, field: &TagField, value: Option<String>| {
            assign_value(
                tags,
                field,
                value,
                &locked,
                touched.as_deref_mut(),
                &mut run.blocked,
            )
        };
        let mut not_extracted = false;
        match step {
            FieldStep::Format(field, mask) => {
                let file = self.file_context(mask, track);
                let value = mask.render_value_with(tags, &file)?;
                assign(tags, field, Some(value));
            }
            FieldStep::Select { fields, keep } => {
                let doomed: Vec<TagField> = tags
                    .keys()
                    .filter(|stored| fields.iter().any(|named| same_field(named, stored)) != *keep)
                    .cloned()
                    .collect();
                for field in doomed {
                    assign(tags, &field, None);
                }
            }
            FieldStep::Extract(source, pattern) => {
                let file = self.file_context(source, track);
                let subject = source.render_value_with(tags, &file)?;
                match pattern.extract(&subject) {
                    Ok(extracted) => {
                        for (field, value) in &extracted {
                            let value = normalize_extracted(field, value);
                            if !value.is_empty() {
                                assign(tags, field, Some(value));
                            }
                        }
                    }
                    Err(MaskError::NoMatch) => not_extracted = true,
                    Err(err) => return Err(err.into()),
                }
            }
            FieldStep::Split(field, delimiter) => {
                if let Some(value) = tags.get(field) {
                    let parts: Vec<String> = values_of(value)
                        .iter()
                        .flat_map(|part| part.split(delimiter.as_str()))
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(str::to_string)
                        .collect();
                    let joined = join_values(&parts);
                    assign(tags, field, Some(joined));
                }
            }
            FieldStep::Merge(field, joiner) => {
                if let Some(value) = tags.get(field) {
                    let merged = values_of(value).join(joiner);
                    assign(tags, field, Some(merged));
                }
            }
            FieldStep::Dedupe(field) => {
                if let Some(value) = tags.get(field) {
                    let mut seen = HashSet::new();
                    let parts: Vec<String> = values_of(value)
                        .into_iter()
                        .filter(|part| seen.insert(part.clone()))
                        .collect();
                    let joined = join_values(&parts);
                    assign(tags, field, Some(joined));
                }
            }
        }
        if not_extracted {
            run.not_extracted += 1;
        }
        Ok(())
    }
}

/// Put `value` in `field`, under the spelling the file already stores it as.
/// Empty removes it. A locked field keeps its value and the attempt is kept in
/// `blocked` instead.
fn assign_value(
    tags: &mut TagMap,
    field: &TagField,
    value: Option<String>,
    locked: &HashSet<String>,
    touched: Option<&mut Vec<TagField>>,
    blocked: &mut Vec<(TagField, Option<String>)>,
) {
    let key = tags
        .keys()
        .find(|stored| same_field(field, stored))
        .cloned()
        .unwrap_or_else(|| field.clone());
    let value = value.filter(|value| !value.is_empty());
    if locked.contains(&key.to_storage_key()) {
        if tags.get(&key) != value.as_ref() {
            blocked.push((key, value));
        }
        return;
    }
    if tags.get(&key) == value.as_ref() {
        return;
    }
    match value {
        Some(value) => {
            tags.insert(key.clone(), value);
        }
        None => {
            tags.remove(&key);
        }
    }
    if let Some(touched) = touched {
        if !touched.contains(&key) {
            touched.push(key);
        }
    }
}

/// The values a stored multi-value string holds, split on the separator in
/// force; whitespace around a part is formatting.
fn values_of(value: &str) -> Vec<String> {
    let separator = tagrex_core::model::multi_value_separator();
    value
        .split(separator.as_str())
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn join_values(parts: &[String]) -> String {
    parts.join(&tagrex_core::model::multi_value_separator())
}
