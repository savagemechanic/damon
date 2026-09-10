from dataclasses import dataclass


@dataclass(frozen=True)
class ExecutionPolicy:
    timeout: float = 60.0
    max_output_bytes: int = 1_000_000
    max_scanned_files: int = 20_000

    def __post_init__(self) -> None:
        if self.timeout <= 0 or self.max_output_bytes <= 0 or self.max_scanned_files <= 0:
            raise ValueError("execution limits must be positive")
