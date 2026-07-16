from __future__ import annotations

from harbor.agents.installed.codex import Codex

from .zvec_grep import ZvecGrepMixin


class ZvecCodex(ZvecGrepMixin, Codex):
    """Harbor agent with zvec-grep provisioned before execution."""
