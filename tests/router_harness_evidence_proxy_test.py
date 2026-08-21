#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("router_harness_evidence_proxy.py")
SPEC = importlib.util.spec_from_file_location("router_harness_evidence_proxy", MODULE_PATH)
assert SPEC and SPEC.loader
proxy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(proxy)


class EvidenceProxyTest(unittest.TestCase):
    def test_additional_tools_inventory_is_bounded_and_recursive(self) -> None:
        body = {
            "input": [
                {
                    "type": "additional_tools",
                    "tools": [
                        {
                            "type": "namespace",
                            "name": "functions",
                            "tools": [
                                {"type": "function", "name": "bash", "parameters": {"secret": "not-recorded"}},
                                {"type": "custom", "name": "exec", "format": {"definition": "not-recorded"}},
                            ],
                        }
                    ],
                }
            ]
        }
        self.assertEqual(
            proxy.bounded_tool_inventory(body),
            {
                "types": ["custom", "function", "namespace"],
                "namespace_count": 1,
                "child_count": 2,
                "length_buckets": ["1-64"],
            },
        )

    def test_inventory_records_only_name_length_classes(self) -> None:
        body = {
            "tools": [
                {"type": "function", "name": "a" * 64},
                {"type": "function", "name": "b" * 65},
                {"type": "function", "name": "c" * 129},
            ]
        }
        inventory = proxy.bounded_tool_inventory(body)
        self.assertEqual(inventory["length_buckets"], ["1-64", "129+", "65-128"])
        self.assertNotIn("a" * 64, str(inventory))
        self.assertNotIn("b" * 65, str(inventory))
        self.assertNotIn("c" * 129, str(inventory))


if __name__ == "__main__":
    unittest.main()
