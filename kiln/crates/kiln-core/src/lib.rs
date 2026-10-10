// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's headless core: config layers and settings, app paths, platform
//! conventions, keymaps and menu tables resolved per platform, theme data
//! and a text-edit model. Must never depend on GPUI, so headless tools load
//! the same config and keymaps as the app.
