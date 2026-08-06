from __future__ import annotations

from harbor.agents.installed.qwen_code import QwenCode

from .zvec_grep import ZvecGrepMixin


class ZvecQwenCode(ZvecGrepMixin, QwenCode):
    """Harbor's Qwen Code agent with zvec-grep provisioned before execution."""
