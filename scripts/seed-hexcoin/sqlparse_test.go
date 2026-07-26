package main

import (
	"testing"
)

func TestParseSQLRows_ProductArray(t *testing.T) {
	raw := []byte(`[
	  {
	    "schema": {
	      "elements": [
	        {"name": {"some": "id"}, "algebraic_type": {"U64": []}},
	        {"name": {"some": "address"}, "algebraic_type": {"String": []}}
	      ]
	    },
	    "rows": [
	      [1, "STELOBANK"],
	      [2, "ABCDEFGH"]
	    ]
	  }
	]`)

	rows, err := parseSQLRows(raw)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 2 {
		t.Fatalf("want 2 rows, got %d: %#v", len(rows), rows)
	}
	id, ok := asUint64(rows[0]["id"])
	if !ok || id != 1 {
		t.Fatalf("row0 id: ok=%v id=%d row=%v", ok, id, rows[0])
	}
	if rows[0]["address"] != "STELOBANK" {
		t.Fatalf("address: %v", rows[0]["address"])
	}
	id2, ok := asUint64(rows[1]["id"])
	if !ok || id2 != 2 {
		t.Fatalf("row1 id: ok=%v id=%d", ok, id2)
	}
}

func TestParseSQLRows_ObjectRows(t *testing.T) {
	raw := []byte(`[{"schema":{"elements":[{"name":"id"},{"name":"address"}]},"rows":[{"id":7,"address":"X"}]}]`)
	rows, err := parseSQLRows(raw)
	if err != nil {
		t.Fatal(err)
	}
	id, ok := asUint64(rows[0]["id"])
	if !ok || id != 7 {
		t.Fatalf("id=%d ok=%v row=%v", id, ok, rows[0])
	}
}
