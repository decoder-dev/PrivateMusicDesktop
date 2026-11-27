package com.dec.privatemusic.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

class SessionViewModel(
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    private val mutableState = MutableStateFlow(SessionState())
    val state: StateFlow<SessionState> = mutableState.asStateFlow()

    fun start() {
        if (mutableState.value.signed_in) return
        refresh()
    }

    fun refresh() {
        scope.launch {
            runCatching { platformSessionState() }
                .onSuccess { mutableState.value = it }
        }
    }

    fun close() = scope.cancel()
}
