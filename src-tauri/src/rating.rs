use openskill::prelude::{Env, GameResult, Rating};

#[cfg(test)]
use crate::models::{DEFAULT_MU, DEFAULT_SIGMA};

pub const MODEL_VERSION: &str = "openskill-0.0.1/plackett-luce";

#[cfg(test)]
pub fn default_rating() -> Rating {
    Rating::new(DEFAULT_MU, DEFAULT_SIGMA)
}

pub fn rate_win(
    winner_mu: f64,
    winner_sigma: f64,
    loser_mu: f64,
    loser_sigma: f64,
) -> Result<(Rating, Rating), String> {
    let environment = Env::default();
    let result = GameResult::new(
        vec![
            vec![Rating::new(winner_mu, winner_sigma)],
            vec![Rating::new(loser_mu, loser_sigma)],
        ],
        vec![1, 2],
    );
    let updated = environment
        .rate(&result)
        .map_err(|error| error.to_string())?;
    Ok((updated[0][0].clone(), updated[1][0].clone()))
}

pub fn ordinal(mu: f64, sigma: f64) -> f64 {
    Env::default().ordinal(&Rating::new(mu, sigma))
}

pub fn predicted_balance(first: &Rating, second: &Rating) -> f64 {
    let teams = vec![vec![first.clone()], vec![second.clone()]];
    Env::default()
        .predict_win(&teams)
        .ok()
        .and_then(|probabilities| probabilities.first().copied())
        .map(|probability| (probability - 0.5).abs())
        .unwrap_or(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winner_moves_above_loser() {
        let initial = default_rating();
        let (winner, loser) =
            rate_win(initial.mu, initial.sigma, initial.mu, initial.sigma).unwrap();

        assert!(winner.mu > initial.mu);
        assert!(loser.mu < initial.mu);
        assert!(ordinal(winner.mu, winner.sigma) > ordinal(loser.mu, loser.sigma));
    }

    #[test]
    fn equal_ratings_predict_a_balanced_match() {
        let first = default_rating();
        let second = default_rating();
        assert!(predicted_balance(&first, &second) < 1e-12);
    }
}
