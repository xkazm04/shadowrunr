# SPDX-License-Identifier: Apache-2.0
"""Malakor's 3D head, built offline from the owner's portrait.

The stream never runs any of this: an image-to-3D model makes a skull once, the build cuts its
jaw, places its anchors and bakes its texture, and the stage's ``lich3d`` face draws the GLB.
"""
