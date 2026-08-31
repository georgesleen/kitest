import kitest

def test_dcsupply_constructs():
    supply = kitest.DcSupply("vin", 5.0)
    assert type(supply).__name__ == "DcSupply"
