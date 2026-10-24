import QtQuick
import QtTest
import "../lock"
import "../theme"

// The lock screen's prompt as the Authenticator drives it: dots for what's
// typed (never the text), the placeholder, the spinner and a wrong password.
Item {
    width: 600
    height: 400

    TestCase {
        name: "LockPrompt"
        when: windowShown

        Component {
            id: promptComponent
            LockPrompt {
                userName: "luna"
                typed: 0
                checking: false
                failed: false
                message: ""
            }
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                fgDim: "#9A9A9A",
                primary: "#FD7014",
                error: "#EE5396",
                backdrop: "#000000"
            });
        }

        function texts(item) {
            const found = [];
            const walk = node => {
                if (node.text !== undefined && node.visible)
                    found.push(node.text);
                for (let i = 0; i < node.children.length; i++)
                    walk(node.children[i]);
            };
            walk(item);
            return found;
        }

        function named(node, name) {
            if (node.objectName === name)
                return node;
            for (let i = 0; i < node.children.length; i++) {
                const found = named(node.children[i], name);
                if (found)
                    return found;
            }
            return null;
        }

        function dots(item) {
            let count = 0;
            const walk = node => {
                if (node.width === Tokens.lockDotSize && node.height === Tokens.lockDotSize && node.color !== undefined)
                    count++;
                for (let i = 0; i < node.children.length; i++)
                    walk(node.children[i]);
            };
            walk(item);
            return count;
        }

        function test_shows_the_initial_the_name_and_a_placeholder() {
            const prompt = createTemporaryObject(promptComponent, parent);
            const shown = texts(prompt);
            verify(shown.includes("L"), shown);
            verify(shown.includes("luna"), shown);
            verify(shown.includes("Password"), shown);
            compare(dots(prompt), 0);
        }

        function test_a_dot_per_character_typed_up_to_the_fields_width() {
            const prompt = createTemporaryObject(promptComponent, parent, {
                typed: 5
            });
            compare(dots(prompt), 5);
            verify(!texts(prompt).includes("Password"));
            prompt.typed = 500;
            verify(dots(prompt) > 5 && dots(prompt) < 500, "capped to what fits");
        }

        function test_a_wrong_password_says_so_and_shakes_back_into_place() {
            const prompt = createTemporaryObject(promptComponent, parent, {
                failed: true,
                message: "Wrong password"
            });
            verify(texts(prompt).includes("Wrong password"));
            const field = named(prompt, "field");
            prompt.shake();
            tryVerify(() => field.x !== 0, Tokens.durationShake, "it moves");
            tryCompare(field, "x", 0, Tokens.durationShake * 2);
        }
    }
}
