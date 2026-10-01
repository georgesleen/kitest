"""Waveform viewing for a transient run."""

_UNITS = ((1e-12, "ps"), (1e-9, "ns"), (1e-6, "us"), (1e-3, "ms"), (1.0, "s"))


def _time_unit(span):
    """Scale and label for a time axis spanning `span` seconds."""
    for scale, label in _UNITS:
        if span < scale * 1000.0:
            return scale, label
    return 1.0, "s"


def scope(result, *nodes, save=None, title=None):
    """Plot transient nodes, or every node when none is named.

    Returns the path written when `save` is given, otherwise None after
    showing the window. Raises ImportError when matplotlib is absent and
    KeyError when a named node is not in the run.
    """
    try:
        import matplotlib
    except ModuleNotFoundError as exc:
        if exc.name != "matplotlib":
            raise
        raise ModuleNotFoundError(
            "kitest.scope needs matplotlib: pip install 'kitest[plot]'",
            name="matplotlib",
        ) from exc

    if save is not None:
        matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    names = list(nodes) or result.nodes()
    if not names:
        raise ValueError("run holds no nodes to plot")

    signals = [(name, result.node(name)) for name in names]
    span = max(signal.time()[-1] for _, signal in signals)
    scale, unit = _time_unit(span)

    figure, axes = plt.subplots()
    for name, signal in signals:
        axes.plot(
            [t / scale for t in signal.time()], signal.values(), label=name
        )
    axes.set_xlabel("time ({})".format(unit))
    axes.set_ylabel("volts")
    axes.grid(True, alpha=0.3)
    if len(signals) > 1:
        axes.legend()
    if title is not None:
        axes.set_title(title)
    figure.tight_layout()

    if save is not None:
        figure.savefig(save, dpi=120)
        plt.close(figure)
        return save

    plt.show()
    return None
