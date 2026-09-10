from damon.providers.catalog import FALLBACK_MODELS, ModelCatalog
from damon.providers.zen import ModelInfo


class Provider:
    def list_models(self): return [ModelInfo("z", "Z", ("low",))]


def test_catalog_uses_fallback_offline_and_caches_refresh(tmp_path):
    catalog = ModelCatalog(tmp_path / "models.json")
    assert catalog.load() == FALLBACK_MODELS
    assert catalog.refresh(Provider())[0].reasoning_efforts == ("low",)
    assert catalog.load()[0].id == "z"
