---
description: "Build a multipage React website about cats. Use to compare regular agent edits vs structural CST edits via cst-mcp-server tools."
mode: "agent"
---

# Cats React Website

Build a complete multipage React (Vite + JSX) website about cats inside `_tmp_workspaces/cats-react/`.

## Site Requirements

The site must have **four pages**:

| Route | Page | Content |
|-------|------|---------|
| `/` | Home | Hero section, tagline, featured cat image, brief intro paragraph |
| `/breeds` | Breeds | Grid of at least 6 cat breeds with name, image placeholder, and a one-sentence description |
| `/facts` | Fun Facts | Numbered list of at least 10 interesting cat facts |
| `/contact` | Contact | Simple form with name, email, and message fields (no backend required) |

## Technical Requirements

- **Vite + React** — use the standard Vite JSX template structure
- **React Router v6** — `BrowserRouter`, `Routes`, `Route` for navigation
- **Shared layout** — a persistent `<Navbar>` with links to all four pages
- **CSS modules or plain CSS** — one stylesheet per component/page, no Tailwind
- **No TypeScript** — plain `.jsx` files only
- **No external UI libraries** — hand-written HTML/CSS only

## File Structure to Create

```
_tmp_workspaces/cats-react/
  index.html
  package.json            ← include react, react-dom, react-router-dom, vite
  vite.config.js
  src/
    main.jsx
    App.jsx
    App.css
    components/
      Navbar.jsx
      Navbar.css
    pages/
      Home.jsx
      Breeds.jsx
      Facts.jsx
      Contact.jsx
      Pages.css          ← shared page styles
```

## Instructions

1. Create every file listed above with working, complete content — no placeholder stubs.
2. `package.json` must include correct `scripts` (`dev`, `build`, `preview`) and all needed dependencies.
3. Each page component must `import` and render `<Navbar />`.
4. The `<Navbar>` must use `<NavLink>` from `react-router-dom` so the active link gets a visible highlight.
5. The Contact form must have basic client-side validation (required fields) using controlled inputs.
6. All cat images can use `https://placekitten.com/<width>/<height>` as src values.

## What to Report

After creating all files, list:
- Each file path created
- The React Router route structure
- Any decisions made about CSS/layout
