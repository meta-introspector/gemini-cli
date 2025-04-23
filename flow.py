# visualize_flow_radial.py
import graphviz
import os
import math

# Ensure the output directory exists
output_dir = "diagrams"
os.makedirs(output_dir, exist_ok=True)
output_path = os.path.join(output_dir, "query_flow_radial")

# Create graph with neato engine which allows explicit positioning
dot = graphviz.Graph(  # Undirected graph for node positioning
    'QueryFlowRadial',
    comment='End-to-End Query Flow (Radial)',
    engine='neato',    # neato allows explicit node positioning
    graph_attr={
        'overlap': 'false',
        'splines': 'true',
        'fontname': 'Arial',
        'fontsize': '14',
        'bgcolor': 'white',
    },
    node_attr={
        'fontname': 'Arial',
        'fontsize': '12',
        'margin': '0.3,0.2',
        'style': 'filled',
    },
    edge_attr={
        'fontname': 'Arial',
        'fontsize': '10',
        'len': '1.5',
    }
)

# Position nodes in a circle around HAPPE
# HAPPE at center (0,0)
dot.node('happe', 'HAPPE\nDaemon', shape='circle', pos='0,0!', 
         width='1.5', height='1.5', fillcolor='#FFF2CC', style='filled,bold')

# Position other nodes in a circle around HAPPE with radius=5
radius = 5
nodes = ['user', 'llm', 'ida', 'mem_mcp', 'other_mcp']
for i, node_id in enumerate(nodes):
    angle = 2 * math.pi * i / len(nodes)
    x = radius * math.cos(angle)
    y = radius * math.sin(angle)
    
    # Position the nodes with fixed positions
    if node_id == 'user':
        dot.node(node_id, 'End User', shape='box', fillcolor='#D4E8FC', 
                 pos=f"{x},{y}!")
    elif node_id == 'llm':
        dot.node(node_id, 'Main LLM\nAPI', shape='box', fillcolor='#ADD8E6', 
                 pos=f"{x},{y}!")
    elif node_id == 'ida':
        dot.node(node_id, 'IDA\nDaemon', shape='box', fillcolor='#D8BFD8', 
                 pos=f"{x},{y}!")
    elif node_id == 'mem_mcp':
        dot.node(node_id, 'Memory\nMCP Server', shape='cylinder', fillcolor='#FFDAB9', 
                 pos=f"{x},{y}!")
    elif node_id == 'other_mcp':
        dot.node(node_id, 'Other MCP\nServer(s)', shape='component', fillcolor='#E0FFE0', 
                 pos=f"{x},{y}!")

# Connect nodes with edges (no labels here, just for positioning)
for node_id in nodes:
    dot.edge('happe', node_id)

# Now create a directed graph for the actual flow
flow = graphviz.Digraph(
    'QueryFlowLogical',
    comment='End-to-End Query Flow (Logical)',
    engine='dot',
    node_attr={'fontname': 'Arial', 'fontsize': '12', 'margin': '0.3,0.2', 'style': 'filled'},
    edge_attr={'fontname': 'Arial', 'fontsize': '10'},
)

# Define nodes
flow.node('happe', 'HAPPE\nDaemon', shape='circle', fillcolor='#FFF2CC', style='filled,bold')
flow.node('user', 'End User', shape='box', fillcolor='#D4E8FC')
flow.node('ida', 'IDA\nDaemon', shape='box', fillcolor='#D8BFD8')
flow.node('mem_mcp', 'Memory\nMCP Server', shape='cylinder', fillcolor='#FFDAB9')
flow.node('other_mcp', 'Other MCP\nServer(s)', shape='component', fillcolor='#E0FFE0')
flow.node('llm', 'Main LLM\nAPI', shape='box', fillcolor='#ADD8E6')

# Define edges with descriptions and flow
flow.edge('user', 'happe', label='1. Query', color='black')
flow.edge('happe', 'ida', label='2. Get Memories', color='black')
flow.edge('ida', 'mem_mcp', label='3. Retrieve', color='black')
flow.edge('mem_mcp', 'ida', label='4. Memories', color='black', style='dashed')
flow.edge('ida', 'happe', label='5. Memories', color='black', style='dashed')
flow.edge('happe', 'llm', label='6. Generate', color='black')
flow.edge('llm', 'happe', label='7a. Function Calls', color='blue', style='dashed')
flow.edge('happe', 'other_mcp', label='7b. Tool Call', color='blue')
flow.edge('other_mcp', 'happe', label='7c. Result', color='blue', style='dashed')
flow.edge('happe', 'llm', label='7d. Tool Response', color='blue')
flow.edge('llm', 'happe', label='8. Response', color='black', style='dashed')
flow.edge('happe', 'user', label='9. Response', color='black', style='dashed')
flow.edge('happe', 'ida', label='10. Store Turn', color='darkgreen', style='dotted')
flow.edge('ida', 'mem_mcp', label='11. Store Memory', color='darkgreen', style='dotted')
flow.edge('mem_mcp', 'ida', label='12. Ack', color='darkgreen', style='dotted,dashed')

try:
    # Render both visualizations
    dot.render(f"{output_path}_positions", format='png', view=False)
    flow.render(f"{output_path}_flow", format='png', view=False)
    print(f"Radial position chart: {output_path}_positions.png")
    print(f"Logical flow chart: {output_path}_flow.png")
except Exception as e:
    print(f"Error during rendering: {e}")
    dot.save(f"{output_path}_positions")
    flow.save(f"{output_path}_flow")