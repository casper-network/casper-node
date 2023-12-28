import argparse
import json


class FixedSequence:
    length: int
    decl: str


class Tuple:
    items: list[str]

def load_and_process_json(filename):
    print('from enum import Enum')
    print('from dataclasses import dataclass')

    print()

    with open(filename, 'r') as f:
        data = json.load(f)

        # initialize empty set
        decls = set()

        mapping = {}
        print('''class Struct:''')
        print('''    def serialize(self) -> bytes:''')
        print('''        pass''')


        print('''class Tuple:
    items: list[str]
''')

        for entry_point in data['entry_points']:
            for argument in entry_point['arguments']:
                decls.add(argument['decl'])
                if argument['decl'] not in data['definitions']:
                    raise KeyError(f'Argument {argument} not found in definitions.')

            if entry_point['result'] not in data['definitions']:
                decls.add(entry_point['result'])
                raise KeyError(f'Result {entry_point["result"]} not found in definitions.')

        for (index, (decl, definition)) in enumerate(data['definitions'].items()):
            if tuple_def := definition.get('Tuple'):
                enum_name = f'Tuple{index}'
                mapping[decl] = enum_name
                print(f'class {enum_name}(Tuple):')
                print(f'    """Generated tuple code for {decl}"""')
                print(f'    items = [')
                for item in tuple_def['items']:
                    if item not in data['definitions']:
                        raise KeyError(f'Item {item} not found in definitions.')
                    print(f'    {decl!r},')
                print(f'   ]')

            elif enum_def := definition.get('Enum'):
                path = decl.split('::')
                enum_name = path[-1]
                if '<' in enum_name or '>' in enum_name:
                    enum_name = f'Enum{index}'
                mapping[decl] = enum_name
                # print(f'class {enum_name}(Enum):')
                # print(f'    """Generated enum code for {decl}"""')
                variants = []
                for item in enum_def['items']:
                    variant_name = f'{enum_name}_{item["name"]}'
                    print(f'@dataclass')
                    print(f'class {variant_name}(Struct):')
                    print(f'    """Generated enum variant for {decl} variant {item["name"]}"""')
                    print(f'    discriminant = {item["discriminant"]}')
                    variants += [variant_name]

                print(f'@dataclass')
                print(f'class {enum_name}:')
                print(f'    """Generated enum code for {decl}"""')
                print('    variant = {}'.format(' | '.join(variants)))

            decls.add(decl)

    print('DECLS = {}')
    for (decl, mapped_type) in mapping.items():
        print(f'DECLS[{decl!r}] = {mapped_type}')

def main():
    parser = argparse.ArgumentParser(description='Process a JSON file.')
    parser.add_argument('filename', help='The filename of the JSON file to process.')
    args = parser.parse_args()

    load_and_process_json(args.filename)

if __name__ == '__main__':
    main()
