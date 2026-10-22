package com.dec.privatemusic.vklogin

import org.json.JSONArray
import org.json.JSONObject

/**
 * What the Activity reports to Rust. The wire format (`event` + camelCase
 * fields) is the one `LoginEvent::from_json` parses in `src/lib.rs`.
 */
sealed class LoginEvent {
    abstract fun toJson(): JSONObject

    object Cancelled : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "cancelled")
    }

    object LoadFailed : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "loadFailed")
    }

    object LoadOk : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "loadOk")
    }

    data class Pending(val reason: String = "unauthorized") : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "pending").put("reason", reason)
    }

    data class Error(val kind: String) : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "error").put("kind", kind)
    }

    data class SmokeLoaded(val host: String) : LoginEvent() {
        override fun toJson() = JSONObject().put("event", "smokeLoaded").put("host", host)
    }

    /** Secrets inside: never printed, only handed to Rust. */
    class Success(
        private val cookieHeader: String,
        private val webTokenJson: String,
        private val setCookies: List<String>,
        private val userAgent: String,
    ) : LoginEvent() {
        override fun toJson(): JSONObject = JSONObject()
            .put("event", "success")
            .put("cookieHeader", cookieHeader)
            .put("webTokenJson", webTokenJson)
            .put("setCookies", JSONArray(setCookies))
            .put("userAgent", userAgent)

        override fun toString() = "Success(<redacted>)"
    }
}
