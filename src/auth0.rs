use actix_web::web::Data;
use actix_web::{dev::Payload, FromRequest, HttpRequest};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use alcoholic_jwt::{token_kid, validate, Validation, ValidationError, JWKS};
use futures::Future;
use reqwest::Client;
use std::env;
use std::pin::Pin;
use tokio::sync::Mutex;

use crate::errors::Error;

#[derive(Debug)]
/// Auth0 authorization material
pub struct Auth {
    pub authz_id: String,
    pub bearer: String,
}

/// Auth0 settings
pub struct Auth0 {
    domain: String,
    jwks: Mutex<Option<JWKS>>,
}

impl Auth0 {
    pub fn from_env() -> Self {
        Self {
            domain: env::var("AUTH0_DOMAIN").expect(
                "Please set the `AUTH0_DOMAIN` environment variable. Example: \
                \"dev--y3j-dq2.us.auth0.com\"",
            ),
            jwks: Mutex::new(None),
        }
    }

    fn base_url(&self) -> String {
        format!("https://{}", self.domain)
    }

    pub async fn validate_token(&self, token: &str) -> Result<Auth, Error> {
        let mut maybe_jwks = self.jwks.lock().await;

        let jwks = match maybe_jwks.as_ref() {
            Some(jwks) => jwks,
            None => {
                let jwks: JWKS = Client::default()
                    .get(format!("{}/.well-known/jwks.json", self.base_url()))
                    .send()
                    .await
                    .map_err(Error::CannotFetchJwks)?
                    .json()
                    .await
                    .map_err(Error::CannotFetchJwksResponse)?;

                maybe_jwks.insert(jwks)
            }
        };

        let kid = match token_kid(token) {
            Ok(Some(kid)) => Ok(kid),
            Ok(None) => Err(Error::JwksNoKid),
            Err(validation_err) => Err(Error::JwksValidationError(validation_err)),
        }?;

        let jwk = jwks
            .find(&kid)
            .ok_or(Error::TokenKidNotFoundInJwksKeysSet)?;

        let res = validate(
            token,
            jwk,
            vec![
                Validation::Issuer(format!("{}/", self.base_url())),
                Validation::SubjectPresent,
                Validation::NotExpired,
            ],
        )
        .map_err(|error| match error {
            ValidationError::InvalidClaims(ref errors) => {
                // Hard-coded string inside JWT lib
                // https://docs.rs/alcoholic_jwt/latest/src/alcoholic_jwt/lib.rs.html#486
                if errors.contains(&"token has expired") {
                    Error::TokenExpired
                } else {
                    Error::JwksValidationError(error)
                }
            }
            _ => Error::JwksValidationError(error),
        })?;

        let sub = res
            .claims
            .get("sub")
            .ok_or(Error::MissingSubInJwtToken)?
            .as_str()
            .ok_or(Error::InvalidSubInJwtToken)?;

        Ok(Auth {
            authz_id: sub.to_owned(),
            bearer: token.to_owned(),
        })
    }
}

impl FromRequest for Auth {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let req = req.clone();
        let auth0 = req.app_data::<Data<Auth0>>().cloned();

        Box::pin(async move {
            let bearer = BearerAuth::extract(&req).into_inner()?;

            auth0
                .ok_or(Error::InvalidConfiguration)?
                .validate_token(bearer.token())
                .await
        })
    }
}
