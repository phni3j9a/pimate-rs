import json
import tempfile
import unittest
from pathlib import Path
from compare import captures


class Comparisons(unittest.TestCase):
    def test_all_three_manifest_entries_must_be_unique_unless_recovering(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            def record(variant, count):
                folder = root / variant
                folder.mkdir(exist_ok=True)
                shot = {'suggestedHumanReadableName': f'Table-{variant}_0.png', 'exportedFileName': 'frame.png'}
                (folder / 'manifest.json').write_text(json.dumps([{'attachments': [shot] * count}]))

            record('light', 1)
            record('dark', 1)
            for count in [None, 0, 2]:
                if count is not None:
                    record('large', count)
                with self.assertRaisesRegex(ValueError, 'Table-large'):
                    captures(root, 'Table', partial=False)
                self.assertEqual(captures(root, 'Table', partial=True), [])
            record('large', 1)
            self.assertEqual(len(captures(root, 'Table', partial=False)), 3)


if __name__ == '__main__':
    unittest.main()
