//! TODO copy tokenizer from https://github.com/latitudegames/GPT-3-Encoder/blob/master/Encoder.js

/// count the number of gpt3 tokens in the input
/// for now we just retrun a guess and hope for the best
pub fn count_tokens(a: &str) -> usize {
    a.len() * 2
}
