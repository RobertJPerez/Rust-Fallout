import unittest

from verify_material_pixel_decoding import compare_rgba8


class CompareRgba8Tests(unittest.TestCase):
    def test_exact_planes_match(self):
        result = compare_rgba8(bytes([10, 20, 30, 255]), bytes([10, 20, 30, 255]), 1, 1)
        self.assertTrue(result["exact_match"])
        self.assertTrue(result["within_one_8bit_code_value"])
        self.assertEqual(result["pixels_with_any_difference"], 0)

    def test_single_code_value_rounding_difference_is_recorded(self):
        result = compare_rgba8(bytes([24, 24, 24, 255]), bytes([25, 24, 25, 255]), 1, 1)
        self.assertFalse(result["exact_match"])
        self.assertTrue(result["within_one_8bit_code_value"])
        self.assertEqual(result["differing_components_rgba"], [1, 0, 1, 0])
        self.assertEqual(result["max_abs_channel_difference"], 1)

    def test_two_code_value_difference_exceeds_tolerance(self):
        result = compare_rgba8(bytes([10, 20, 30, 255]), bytes([12, 20, 30, 255]), 1, 1)
        self.assertFalse(result["within_one_8bit_code_value"])
        self.assertEqual(result["max_abs_channel_difference"], 2)

    def test_rejects_wrong_rgba_plane_size(self):
        with self.assertRaisesRegex(ValueError, "RGBA plane size mismatch"):
            compare_rgba8(bytes(4), bytes(8), 1, 1)

    def test_rejects_nonpositive_dimensions(self):
        with self.assertRaisesRegex(ValueError, "dimensions must be positive"):
            compare_rgba8(b"", b"", 0, 1)


if __name__ == "__main__":
    unittest.main()
