use std::{
    fmt::{Display, Formatter},
    string::FromUtf8Error,
};

use actix_web::{
    error::ResponseError,
    http::{header::ContentType, StatusCode},
    web::Json,
    HttpResponse,
};
use actix_web_httpauth::{
    extractors::AuthenticationError, headers::www_authenticate::bearer::Bearer,
};
use alcoholic_jwt::ValidationError;
use cosmian_findex::error::FindexErr;
use reqwest::header::InvalidHeaderValue;

pub type Response<T> = Result<Json<T>, Error>;
pub type ResponseBytes = Result<HttpResponse, Error>;

#[derive(Debug)]
pub enum Error {
    Internal,
    InvalidSignature,
    WrongEncoding,
    Json,
    WrongIndexPublicId,
    Findex(String),

    InvalidConfiguration,

    CannotFetchJwks(reqwest::Error),
    CannotFetchJwksResponse(reqwest::Error),

    JwksNoKid,
    JwksValidationError(ValidationError),
    TokenKidNotFoundInJwksKeysSet,
    MissingSubInJwtToken,
    InvalidSubInJwtToken,
    TokenExpired,

    FailToBuildBearerHeader(InvalidHeaderValue),
    BearerError(AuthenticationError<Bearer>),

    UnknownProject(String),

    Reqwest(reqwest::Error),

    BadRequest(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")?;

        Ok(())
    }
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        log::error!("{self:?}");

        match *self {
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidSignature => StatusCode::FORBIDDEN,
            Self::WrongEncoding => StatusCode::BAD_REQUEST,
            Self::Json => StatusCode::BAD_REQUEST,
            Self::WrongIndexPublicId => StatusCode::BAD_REQUEST,
            Self::Findex(_) => StatusCode::BAD_REQUEST,

            Self::InvalidConfiguration => StatusCode::INTERNAL_SERVER_ERROR,

            Self::CannotFetchJwks(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::CannotFetchJwksResponse(_) => StatusCode::INTERNAL_SERVER_ERROR,

            Self::JwksNoKid => StatusCode::INTERNAL_SERVER_ERROR,
            Self::JwksValidationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::TokenKidNotFoundInJwksKeysSet => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingSubInJwtToken => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidSubInJwtToken => StatusCode::INTERNAL_SERVER_ERROR,
            Self::TokenExpired => StatusCode::FORBIDDEN,

            Self::FailToBuildBearerHeader(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::BearerError(_) => StatusCode::FORBIDDEN,

            Self::UnknownProject(_) => StatusCode::NOT_FOUND,
            Self::Reqwest(_) => StatusCode::INTERNAL_SERVER_ERROR,

            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Error::Internal
    }
}

impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Error::Json
    }
}

impl From<FromUtf8Error> for Error {
    fn from(_: FromUtf8Error) -> Self {
        Error::WrongEncoding
    }
}

impl From<FindexErr> for Error {
    fn from(err: FindexErr) -> Self {
        Error::Findex(err.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Reqwest(err)
    }
}

impl From<InvalidHeaderValue> for Error {
    fn from(err: InvalidHeaderValue) -> Self {
        Error::FailToBuildBearerHeader(err)
    }
}

impl From<AuthenticationError<Bearer>> for Error {
    fn from(err: AuthenticationError<Bearer>) -> Self {
        Error::BearerError(err)
    }
}
