/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["web/**/*.{html,go}"],
  theme: {
    extend: {
      animation: {
        "spin-slow": "spin 3s linear infinite"
      },
      height: {
        "screen-available": "calc(100dvh - var(--header-offset))",
        header: "var(--header-offset)"
      },
      fontFamily: {
        "source-code-pro": ["Source Code Pro", "monospace"]
      },
      colors: {
        neutral: {
          950: "#111111"
        },
        melrose: {
          50: "#edf4ff",
          100: "#dfe9ff",
          200: "#c5d7ff",
          DEFAULT: "#a2bbff",
          400: "#7d95fc",
          500: "#5e70f6",
          600: "#4148ea",
          700: "#3336cf",
          800: "#2c30a7",
          900: "#2b3184"
        },
        anakiwa: {
          50: "#f0fbff",
          100: "#dff5ff",
          DEFAULT: "#a2e9ff",
          300: "#79e2ff",
          400: "#32d5fe",
          500: "#07c0f0",
          600: "#009ccd",
          700: "#007ca6",
          800: "#036889",
          900: "#095671"
        }
      }
    }
  },
  plugins: [],
}

