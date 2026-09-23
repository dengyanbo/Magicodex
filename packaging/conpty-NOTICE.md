## Windows pseudo console (conpty.dll, OpenConsole.exe)

`conpty.dll` and `OpenConsole.exe` are unmodified copies from the Microsoft NuGet package
[Microsoft.Windows.Console.ConPTY](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY)
1.24.260710001 (`runtimes/win-x64/native/conpty.dll`, `build/native/runtimes/x64/OpenConsole.exe`),
built from [microsoft/terminal](https://github.com/microsoft/terminal) and signed by Microsoft
Corporation. Package SHA256: `175640566A3B59C4B132070EE96C2C77E5AB7EDD2E92732A5EB3610BBF63D90E`.

magicopilot loads this pseudo console instead of the one built into Windows, the same way
Windows Terminal does, so that GitHub Copilot CLI talks to the real terminal and draws the
same interface as when it runs directly.

```text
MIT License

Copyright (c) Microsoft Corporation. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE
```

## GitHub Copilot CLI

magicopilot does not contain, modify or redistribute GitHub Copilot CLI. It starts the copy
the user installed, unchanged, and draws next to it; Copilot CLI remains subject to its own
license and terms.
