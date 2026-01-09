mod check_authorization;
mod complete_database_name;
mod complete_user_name;
mod create_databases;
mod create_users;
mod drop_databases;
mod drop_users;
mod list_all_databases;
mod list_all_privileges;
mod list_all_users;
mod list_databases;
mod list_privileges;
mod list_users;
mod list_valid_name_prefixes;
mod lock_users;
mod modify_privileges;
mod passwd_user;
mod unlock_users;

use std::collections::BTreeSet;

pub use check_authorization::*;
pub use complete_database_name::*;
pub use complete_user_name::*;
pub use create_databases::*;
pub use create_users::*;
pub use drop_databases::*;
pub use drop_users::*;
pub use list_all_databases::*;
pub use list_all_privileges::*;
pub use list_all_users::*;
pub use list_databases::*;
pub use list_privileges::*;
pub use list_users::*;
pub use list_valid_name_prefixes::*;
pub use lock_users::*;
pub use modify_privileges::*;
pub use passwd_user::*;
pub use unlock_users::*;

use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
use tokio_serde::{Framed as SerdeFramed, formats::Bincode};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::core::types::{MySQLDatabase, MySQLUser};

pub type ServerToClientMessageStream = SerdeFramed<
    Framed<UnixStream, LengthDelimitedCodec>,
    Request,
    Response,
    Bincode<Request, Response>,
>;

pub type ClientToServerMessageStream = SerdeFramed<
    Framed<UnixStream, LengthDelimitedCodec>,
    Response,
    Request,
    Bincode<Response, Request>,
>;

const MAX_REQUEST_FRAME_LENGTH: usize = 100 * 1024; // 100 KB
const MAX_RESPONSE_FRAME_LENGTH: usize = 1024 * 1024; // 1 MB

pub fn create_client_to_server_message_stream(socket: UnixStream) -> ClientToServerMessageStream {
    let codec = {
        let mut codec = LengthDelimitedCodec::new();
        codec.set_max_frame_length(MAX_REQUEST_FRAME_LENGTH);
        codec
    };
    let length_delimited = Framed::new(socket, codec);
    tokio_serde::Framed::new(length_delimited, Bincode::default())
}

pub fn create_server_to_client_message_stream(socket: UnixStream) -> ServerToClientMessageStream {
    let codec = {
        let mut codec = LengthDelimitedCodec::new();
        codec.set_max_frame_length(MAX_RESPONSE_FRAME_LENGTH);
        codec
    };
    let length_delimited = Framed::new(socket, codec);
    tokio_serde::Framed::new(length_delimited, Bincode::default())
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    CheckAuthorization(CheckAuthorizationRequest),

    ListValidNamePrefixes,
    CompleteDatabaseName(CompleteDatabaseNameRequest),
    CompleteUserName(CompleteUserNameRequest),

    CreateDatabases(CreateDatabasesRequest),
    DropDatabases(DropDatabasesRequest),
    ListDatabases(ListDatabasesRequest),
    ListPrivileges(ListPrivilegesRequest),
    ModifyPrivileges(ModifyPrivilegesRequest),

    CreateUsers(CreateUsersRequest),
    DropUsers(DropUsersRequest),
    PasswdUser(SetUserPasswordRequest),
    ListUsers(ListUsersRequest),
    LockUsers(LockUsersRequest),
    UnlockUsers(UnlockUsersRequest),

    // Commit,
    Exit,
}

impl Request {
    pub fn command_name(&self) -> &str {
        match self {
            Request::CheckAuthorization(_) => "check-authorization",
            Request::ListValidNamePrefixes => "list-valid-name-prefixes",
            Request::CompleteDatabaseName(_) => "complete-database-name",
            Request::CompleteUserName(_) => "complete-user-name",
            Request::CreateDatabases(_) => "create-databases",
            Request::DropDatabases(_) => "drop-databases",
            Request::ListDatabases(_) => "list-databases",
            Request::ListPrivileges(_) => "list-privileges",
            Request::ModifyPrivileges(_) => "modify-privileges",
            Request::CreateUsers(_) => "create-users",
            Request::DropUsers(_) => "drop-users",
            Request::PasswdUser(_) => "passwd-user",
            Request::ListUsers(_) => "list-users",
            Request::LockUsers(_) => "lock-users",
            Request::UnlockUsers(_) => "unlock-users",
            Request::Exit => "exit",
        }
    }

    pub fn affected_users(&self) -> BTreeSet<MySQLUser> {
        match self {
            Request::CheckAuthorization(_) => Default::default(),
            Request::ListValidNamePrefixes => Default::default(),
            Request::CompleteDatabaseName(_) => Default::default(),
            Request::CompleteUserName(_) => Default::default(),
            Request::CreateDatabases(_) => Default::default(),
            Request::DropDatabases(_) => Default::default(),
            Request::ListDatabases(_) => Default::default(),
            Request::ListPrivileges(_) => Default::default(),
            Request::ModifyPrivileges(priv_diffs) => priv_diffs
                .iter()
                .map(|priv_diff| priv_diff.get_user_name().clone())
                .collect(),
            Request::CreateUsers(users) => users.iter().cloned().collect(),
            Request::DropUsers(users) => users.iter().cloned().collect(),
            Request::PasswdUser(user_passwd_req) => {
                let mut result = BTreeSet::new();
                result.insert(user_passwd_req.0.clone());
                result
            }
            Request::ListUsers(users) => users.clone().unwrap_or_default().into_iter().collect(),
            Request::LockUsers(users) => users.iter().cloned().collect(),
            Request::UnlockUsers(users) => users.iter().cloned().collect(),
            Request::Exit => Default::default(),
        }
    }

    pub fn affected_databases(&self) -> BTreeSet<MySQLDatabase> {
        match self {
            Request::CheckAuthorization(_) => Default::default(),
            Request::ListValidNamePrefixes => Default::default(),
            Request::CompleteDatabaseName(_) => Default::default(),
            Request::CompleteUserName(_) => Default::default(),
            Request::CreateDatabases(databases) => databases.iter().cloned().collect(),
            Request::DropDatabases(databases) => databases.iter().cloned().collect(),
            Request::ListDatabases(databases) => {
                databases.clone().unwrap_or_default().into_iter().collect()
            }
            Request::ListPrivileges(databases) => {
                databases.clone().unwrap_or_default().into_iter().collect()
            }
            Request::ModifyPrivileges(priv_diffs) => priv_diffs
                .iter()
                .map(|priv_diff| priv_diff.get_database_name().clone())
                .collect(),
            Request::CreateUsers(_) => Default::default(),
            Request::DropUsers(_) => Default::default(),
            Request::PasswdUser(_) => Default::default(),
            Request::ListUsers(_) => Default::default(),
            Request::LockUsers(_) => Default::default(),
            Request::UnlockUsers(_) => Default::default(),
            Request::Exit => Default::default(),
        }
    }
}

// TODO: include a generic "message" that will display a message to the user?

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    CheckAuthorization(CheckAuthorizationResponse),

    ListValidNamePrefixes(ListValidNamePrefixesResponse),
    CompleteDatabaseName(CompleteDatabaseNameResponse),
    CompleteUserName(CompleteUserNameResponse),

    // Specific data for specific commands
    CreateDatabases(CreateDatabasesResponse),
    DropDatabases(DropDatabasesResponse),
    ListDatabases(ListDatabasesResponse),
    ListAllDatabases(ListAllDatabasesResponse),
    ListPrivileges(ListPrivilegesResponse),
    ListAllPrivileges(ListAllPrivilegesResponse),
    ModifyPrivileges(ModifyPrivilegesResponse),

    CreateUsers(CreateUsersResponse),
    DropUsers(DropUsersResponse),
    SetUserPassword(SetUserPasswordResponse),
    ListUsers(ListUsersResponse),
    ListAllUsers(ListAllUsersResponse),
    LockUsers(LockUsersResponse),
    UnlockUsers(UnlockUsersResponse),

    // Generic responses
    Ready,
    Error(String),
}
