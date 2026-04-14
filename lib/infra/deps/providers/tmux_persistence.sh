#!/usr/bin/env bash
# Placeholder: tmux persistence is now handled by workhive's builtin persistence module.
# This file is retained so the deps provider framework doesn't break.
# See lib/infra/persistence/ for the builtin implementation.

[[ -n "${_DEP_TMUX_PERSISTENCE_LOADED:-}" ]] && return 0
readonly _DEP_TMUX_PERSISTENCE_LOADED=1
