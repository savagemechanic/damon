import os

import pytest

from damon.protocol import FinalAnswer, parse_action
from damon.providers.zen import ZenProvider


KEY = os.environ.get("DAMON_OPENCODE_API_KEY")
MODEL_ID = os.environ.get("DAMON_ZEN_TEST_MODEL")


@pytest.mark.skipif(not (KEY and MODEL_ID), reason="set explicit Zen integration credentials and model")
def test_real_zen_stream_can_finish_protocol_action():
    provider = ZenProvider(KEY)
    model = next(item for item in provider.list_models() if item.id == MODEL_ID)
    messages = [
        {"role": "system", "content": "Return exactly: ACTION: finish followed by a short confirmation."},
        {"role": "user", "content": "Confirm this integration stream."},
    ]
    text = "".join(delta for kind, delta in provider.stream_chat(messages, model) if kind == "text")
    assert isinstance(parse_action(text), FinalAnswer)
