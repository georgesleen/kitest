import kitest
from netlists import load

RC = load("rc.cir")


def run():
    ng = kitest.Ngspice()
    return ng.run_tran(
        RC,
        [kitest.TranSource.pulse("vin", kitest.Pulse.step(low=0.0, high=1.0))],
        kitest.Tran(step=1e-5, stop=5e-3),
    )


def test_scope_writes_the_named_node(tmp_path):
    out = tmp_path / "vout.png"
    assert kitest.scope(run(), "vout", save=str(out)) == str(out)
    assert out.stat().st_size > 1000


def test_scope_without_nodes_plots_every_node(tmp_path):
    result = run()
    one = tmp_path / "one.png"
    every = tmp_path / "every.png"
    kitest.scope(result, "vout", save=str(one))
    kitest.scope(result, save=str(every))

    assert len(result.nodes()) > 1
    assert every.read_bytes() != one.read_bytes()
