import test from 'ava';

const fs = require("fs")
const loader = require("@assemblyscript/loader")

function loadWasmModule(fileName: String) {
    const myImports = {
        env: {
          abort(msgPtr, filePtr, line, column) {
             var msg = msgPtr > 0 ? myModule.__getString(msgPtr) : "";
             var file = myModule.__getString(filePtr);
             console.error(`abort called at ${file}:${line}:${column}: ${msg}`);
          },
        },
      }
      
    let myModule = loader.instantiateSync(
        fs.readFileSync(__dirname + `/../../build/${fileName}.spec.as.wasm`),
        myImports);

    return myModule;
}

export function defineTestsFromModule(moduleName: string) {
    const instance = loadWasmModule(moduleName);
    for (const testName in instance) {
        const testInstance = instance[testName];

        const testId = testName.toLowerCase();
        if (testId.startsWith("test")) {
            test(testName, t => {
                t.truthy(testInstance());
            });
        }
        else if (testId.startsWith("xtest")) {
            test.skip(testName, t => {
                t.truthy(testInstance());
            });
        }
    }
}
