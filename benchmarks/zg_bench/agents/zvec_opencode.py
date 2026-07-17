from __future__ import annotations

from harbor.agents.installed.opencode import OpenCode

from .zvec_grep import ZvecGrepMixin


class ZvecOpenCode(ZvecGrepMixin, OpenCode):
    """Harbor's OpenCode agent with zvec-grep provisioned before execution."""
