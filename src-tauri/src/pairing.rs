use std::collections::{HashMap, HashSet};

use rand::seq::SliceRandom;

use crate::{models::PhotoRecord, rating};

type PairScore = ((u32, u32, f64), (i64, i64));

pub fn next_pair(
    photos: &[PhotoRecord],
    previous_pair_counts: &[((i64, i64), u32)],
    excluded_pair: Option<(i64, i64)>,
) -> Option<(i64, i64)> {
    if photos.len() < 2 {
        return None;
    }

    let seen: HashSet<(i64, i64)> = previous_pair_counts
        .iter()
        .map(|(pair, _)| normalize_pair(*pair))
        .collect();
    let excluded_pair = excluded_pair.map(normalize_pair);
    let mut candidates = photos.to_vec();
    candidates.shuffle(&mut rand::thread_rng());
    candidates.sort_by_key(|photo| photo.comparison_count);

    for first in &candidates {
        let mut opponents: Vec<_> = candidates
            .iter()
            .filter(|second| second.id != first.id)
            .filter(|second| !seen.contains(&normalize_pair((first.id, second.id))))
            .filter(|second| Some(normalize_pair((first.id, second.id))) != excluded_pair)
            .collect();
        if opponents.is_empty() {
            continue;
        }
        opponents.sort_by(|left, right| {
            left.comparison_count
                .cmp(&right.comparison_count)
                .then_with(|| {
                    let first_rating = openskill::prelude::Rating::new(first.mu, first.sigma);
                    let left_rating = openskill::prelude::Rating::new(left.mu, left.sigma);
                    let right_rating = openskill::prelude::Rating::new(right.mu, right.sigma);
                    rating::predicted_balance(&first_rating, &left_rating)
                        .total_cmp(&rating::predicted_balance(&first_rating, &right_rating))
                })
                .then_with(|| left.id.cmp(&right.id))
        });
        return Some((first.id, opponents[0].id));
    }

    let counts: HashMap<_, _> = previous_pair_counts.iter().copied().collect();
    let mut best: Option<PairScore> = None;
    for (index, first) in candidates.iter().take(20).enumerate() {
        for second in candidates.iter().skip(index + 1).take(20) {
            let pair = normalize_pair((first.id, second.id));
            if photos.len() > 2 && Some(pair) == excluded_pair {
                continue;
            }
            let repeat_count = counts.get(&pair).copied().unwrap_or_default();
            let first_rating = openskill::prelude::Rating::new(first.mu, first.sigma);
            let second_rating = openskill::prelude::Rating::new(second.mu, second.sigma);
            let score = (
                repeat_count,
                first.comparison_count + second.comparison_count,
                rating::predicted_balance(&first_rating, &second_rating),
            );
            if best
                .as_ref()
                .map(|(best_score, _)| score < *best_score)
                .unwrap_or(true)
            {
                best = Some((score, pair));
            }
        }
    }
    best.map(|(_, pair)| pair).or(excluded_pair)
}

pub fn normalize_pair(pair: (i64, i64)) -> (i64, i64) {
    if pair.0 <= pair.1 {
        pair
    } else {
        (pair.1, pair.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PhotoRecord;

    fn photo(id: i64, comparison_count: u32) -> PhotoRecord {
        PhotoRecord {
            id,
            file_name: format!("{id}.jpg"),
            thumbnail_path: String::new(),
            mu: 25.0,
            sigma: 8.333,
            comparison_count,
            wins: 0,
            losses: 0,
        }
    }

    #[test]
    fn does_not_repeat_a_pair_while_an_unseen_pair_exists() {
        let photos = vec![photo(1, 1), photo(2, 1), photo(3, 1)];
        let pair = next_pair(&photos, &[((1, 2), 1)], None).unwrap();
        assert_ne!(normalize_pair(pair), (1, 2));
    }

    #[test]
    fn returns_none_for_fewer_than_two_photos() {
        assert_eq!(next_pair(&[photo(1, 0)], &[], None), None);
    }

    #[test]
    fn skip_avoids_immediately_repeating_a_pair() {
        let photos = vec![photo(1, 0), photo(2, 0), photo(3, 0)];
        let pair = next_pair(&photos, &[], Some((1, 2))).unwrap();
        assert_ne!(normalize_pair(pair), (1, 2));
    }
}
