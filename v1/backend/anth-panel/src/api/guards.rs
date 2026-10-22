use super::prelude::*;

pub(crate) async fn require_account_control(state: &AppState) -> Result<(), ApiError> {
    if !state.config().await.accounts.control {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account control is disabled",
        ));
    }
    Ok(())
}

pub(crate) async fn require_account_password_control(state: &AppState) -> Result<(), ApiError> {
    if !state.config().await.accounts.manage_passwords {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "password changes are disabled",
        ));
    }
    Ok(())
}

pub(crate) async fn require_volume_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.volumes.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "volume control is disabled",
        ))
    }
}

pub(crate) async fn require_volume_partition(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.volumes.partition {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "partition edits are disabled",
        ))
    }
}

pub(crate) async fn require_volume_format(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.volumes.format {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, "`mkfs` is disabled"))
    }
}

pub(crate) async fn require_network_manage(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.network.manage {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "network management is disabled",
        ))
    }
}

pub(crate) async fn require_network_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.network.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "network control is disabled",
        ))
    }
}

pub(crate) async fn require_firewall_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.firewall.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "firewall control is disabled",
        ))
    }
}

pub(crate) async fn require_file_write(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.files.write {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "file writes are disabled",
        ))
    }
}

pub(crate) async fn require_plugin_install(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.plugins.install_from_web {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "plugin installation from the browser is disabled",
        ))
    }
}
