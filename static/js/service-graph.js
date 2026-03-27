/**
 * KnowledgeGraph - D3.js force-directed multi-domain graph for dtx
 * Consumes /api/graph?view= endpoints, renders resources, symbols, memories, files
 */

class KnowledgeGraph {
    constructor(container) {
        this.container = container;
        this.currentView = 'knowledge';
        this.impactMode = false;
        this.selectedNode = null;
        this.graphData = null;

        // Design colors
        this.colors = {
            running: '#22c55e',
            starting: '#f59e0b',
            stopped: '#6b7280',
            error: '#ec4899',
            active: '#22d3ee',
            background: '#161b22',
            border: '#2d333b',
            // Domain colors
            resource: '#22c55e',
            symbol: '#a78bfa',
            memory: '#fb923c',
            file: '#4ade80',
            group: '#a78bfa',
            // Edge kind colors
            depends_on: '#2d333b',
            implements: '#a78bfa',
            configures: '#c084fc',
            references: '#fb923c',
            documents: '#fdba74',
            calls: '#60a5fa',
            contains: '#4ade80',
        };

        this.initSvg();
    }

    initSvg() {
        // Create SVG with zoom behavior
        this.container.innerHTML = '';
        const width = this.container.clientWidth;
        const height = this.container.clientHeight || 500;

        this.svg = d3.select(this.container)
            .append('svg')
            .attr('width', '100%')
            .attr('height', '100%')
            .attr('viewBox', [0, 0, width, height]);

        // Defs: grid pattern, glow filter, arrow markers
        const defs = this.svg.append('defs');

        // Grid pattern
        const pattern = defs.append('pattern')
            .attr('id', 'grid')
            .attr('width', 40)
            .attr('height', 40)
            .attr('patternUnits', 'userSpaceOnUse');
        pattern.append('path')
            .attr('d', 'M 40 0 L 0 0 0 40')
            .attr('fill', 'none')
            .attr('stroke', this.colors.border)
            .attr('stroke-width', 0.5)
            .attr('opacity', 0.3);

        // Glow filter
        const filter = defs.append('filter').attr('id', 'glow');
        filter.append('feGaussianBlur').attr('stdDeviation', 3).attr('result', 'coloredBlur');
        const feMerge = filter.append('feMerge');
        feMerge.append('feMergeNode').attr('in', 'coloredBlur');
        feMerge.append('feMergeNode').attr('in', 'SourceGraphic');

        // Arrow marker
        defs.append('marker')
            .attr('id', 'arrow')
            .attr('viewBox', '0 -5 10 10')
            .attr('refX', 25)
            .attr('refY', 0)
            .attr('markerWidth', 6)
            .attr('markerHeight', 6)
            .attr('orient', 'auto')
            .append('path')
            .attr('d', 'M0,-5L10,0L0,5')
            .attr('fill', this.colors.border);

        // Background with grid
        this.svg.append('rect')
            .attr('width', '100%')
            .attr('height', '100%')
            .attr('fill', 'url(#grid)');

        // Container group for zoom/pan
        this.graphContainer = this.svg.append('g').attr('class', 'graph-container');
        this.linkGroup = this.graphContainer.append('g').attr('class', 'links');
        this.nodeGroup = this.graphContainer.append('g').attr('class', 'nodes');

        // Zoom behavior
        this.zoom = d3.zoom()
            .scaleExtent([0.1, 4])
            .on('zoom', (event) => {
                this.graphContainer.attr('transform', event.transform);
            });
        this.svg.call(this.zoom);

        // Tooltip
        this.tooltip = d3.select(this.container)
            .append('div')
            .attr('class', 'dtx-graph-tooltip')
            .style('opacity', 0);
    }

    async init() {
        await this.fetchAndRender(this.currentView);
    }

    async fetchAndRender(view) {
        try {
            const resp = await fetch(`/api/graph?view=${view}`);
            const data = await resp.json();
            this.graphData = data;
            this.currentView = view;
            this.render(data);
        } catch (err) {
            console.error('Failed to fetch graph:', err);
        }
    }

    async setView(view) {
        await this.fetchAndRender(view);
    }

    render(data, opts = {}) {
        // Convert nodes HashMap to array
        const nodesArray = Object.values(data.nodes);
        const edges = data.edges || [];

        // Capture existing node positions for continuity
        const prevPositions = {};
        if (this.simulation) {
            this.simulation.nodes().forEach(n => {
                prevPositions[n.id] = { x: n.x, y: n.y };
            });
        }

        // Clear previous
        this.linkGroup.selectAll('*').remove();
        this.nodeGroup.selectAll('*').remove();

        // Empty state
        if (nodesArray.length === 0) {
            this.nodeGroup.append('text')
                .attr('x', this.container.clientWidth / 2)
                .attr('y', this.container.clientHeight / 2)
                .attr('text-anchor', 'middle')
                .attr('fill', '#6b7280')
                .attr('font-family', 'JetBrains Mono, monospace')
                .attr('font-size', '13px')
                .text(`No ${this.currentView} nodes to display`);
            return;
        }

        // Create link data with node references, reuse positions
        const nodeMap = {};
        nodesArray.forEach(n => {
            const prev = prevPositions[n.id];
            nodeMap[n.id] = { ...n, x: prev?.x || 0, y: prev?.y || 0 };
        });

        const links = edges.map(e => ({
            source: e.source,
            target: e.target,
            kind: e.kind,
            confidence: e.confidence,
        })).filter(e => nodeMap[e.source] && nodeMap[e.target]);

        const nodes = Object.values(nodeMap);

        // Seed new nodes (no previous position) at the expand origin if provided
        if (opts.expandOrigin) {
            nodes.forEach(n => {
                if (!prevPositions[n.id]) {
                    n.x = opts.expandOrigin.x + (Math.random() - 0.5) * 60;
                    n.y = opts.expandOrigin.y + (Math.random() - 0.5) * 60;
                }
            });
        }

        const width = this.container.clientWidth;
        const height = this.container.clientHeight || 500;
        const nodeCount = nodes.length;
        this.nodeCount = nodeCount;

        // Scale forces based on node count
        const charge = nodeCount > 500 ? -30 : nodeCount > 100 ? -100 : -300;
        const collideRadius = nodeCount > 500 ? 8 : nodeCount > 100 ? 20 : 50;
        const linkDistance = nodeCount > 500 ? 30 : nodeCount > 100 ? 60 : 120;

        // Initialize positions in a bounded area to prevent explosion
        // Skip for nodes that already have positions (from previous simulation or expand origin)
        const cx = width / 2, cy = height / 2;
        const initRadius = Math.min(width, height) * 0.4;
        const hasPrev = Object.keys(prevPositions).length > 0;
        nodes.forEach((n, i) => {
            if (hasPrev && (prevPositions[n.id] || opts.expandOrigin)) return;
            const angle = (2 * Math.PI * i) / nodeCount;
            const r = initRadius * Math.sqrt(Math.random());
            n.x = cx + r * Math.cos(angle);
            n.y = cy + r * Math.sin(angle);
        });

        // Force simulation
        if (this.simulation) this.simulation.stop();
        this.simulation = d3.forceSimulation(nodes)
            .force('link', d3.forceLink(links).id(d => d.id).distance(linkDistance))
            .force('charge', d3.forceManyBody().strength(charge))
            .force('center', opts.preserveView ? null : d3.forceCenter(cx, cy))
            .force('collision', d3.forceCollide().radius(collideRadius))
            .force('bounds', () => {
                // Keep nodes within reasonable bounds
                const bound = Math.max(width, height) * 2;
                nodes.forEach(n => {
                    n.x = Math.max(-bound, Math.min(bound, n.x));
                    n.y = Math.max(-bound, Math.min(bound, n.y));
                });
            });

        // For expand: pin existing nodes so only new ones settle into place
        if (opts.preserveView) {
            nodes.forEach(n => {
                if (prevPositions[n.id]) {
                    n.fx = n.x;
                    n.fy = n.y;
                }
            });
            this.simulation.alpha(0.3).alphaDecay(0.08);
            // Unpin after settling
            this.simulation.on('end.unpin', () => {
                nodes.forEach(n => { n.fx = null; n.fy = null; });
                this.simulation.on('end.unpin', null);
            });
        }

        // Draw edges
        this.link = this.linkGroup.selectAll('line')
            .data(links)
            .join('line')
            .attr('stroke', d => this.colors[d.kind] || this.colors.border)
            .attr('stroke-width', d => this.getEdgeWidth(d.confidence))
            .attr('stroke-opacity', d => this.getEdgeOpacity(d.confidence))
            .attr('stroke-dasharray', d => this.getEdgeDash(d.confidence))
            .attr('marker-end', 'url(#arrow)');

        // Draw nodes
        this.node = this.nodeGroup.selectAll('g')
            .data(nodes)
            .join('g')
            .attr('class', 'node')
            .call(this.drag())
            .on('click', (event, d) => this.selectNode(d))
            .on('dblclick', (event, d) => {
                if (d.domain === 'group') {
                    event.stopPropagation();
                    this.expandNode(d.id);
                }
            })
            .on('mouseenter', (event, d) => this.showTooltip(event, d))
            .on('mouseleave', () => this.hideTooltip());

        // Node shapes by domain
        this.node.each((d, i, nodes) => {
            const g = d3.select(nodes[i]);
            const domain = d.domain;

            if (domain === 'group') {
                // Hexagon for group nodes
                const s = 18;
                const hex = [0,1,2,3,4,5].map(i => {
                    const a = Math.PI / 3 * i - Math.PI / 6;
                    return `${s * Math.cos(a)},${s * Math.sin(a)}`;
                }).join(' ');
                const childDomain = d.metadata?.child_domain || 'symbol';
                g.append('polygon')
                    .attr('points', hex)
                    .attr('fill', this.colors.background)
                    .attr('stroke', this.colors[childDomain] || this.colors.symbol)
                    .attr('stroke-width', 2)
                    .attr('class', 'node-shape');
                // Count badge inside
                g.append('text')
                    .attr('dy', 4)
                    .attr('text-anchor', 'middle')
                    .attr('fill', this.colors[childDomain] || this.colors.symbol)
                    .attr('font-size', '10px')
                    .attr('font-weight', 'bold')
                    .attr('font-family', 'JetBrains Mono, monospace')
                    .attr('pointer-events', 'none')
                    .text(d.metadata?.count || '');
            } else if (domain === 'resource') {
                g.append('circle')
                    .attr('r', 20)
                    .attr('fill', this.colors.background)
                    .attr('stroke', this.getResourceColor(d))
                    .attr('stroke-width', 2)
                    .attr('class', 'node-shape');
            } else if (domain === 'symbol') {
                // Diamond
                g.append('polygon')
                    .attr('points', '0,-14 14,0 0,14 -14,0')
                    .attr('fill', this.colors.background)
                    .attr('stroke', this.colors.symbol)
                    .attr('stroke-width', 2)
                    .attr('class', 'node-shape');
            } else if (domain === 'memory') {
                // Rounded rect
                g.append('rect')
                    .attr('x', -14)
                    .attr('y', -10)
                    .attr('width', 28)
                    .attr('height', 20)
                    .attr('rx', 4)
                    .attr('fill', this.colors.background)
                    .attr('stroke', this.colors.memory)
                    .attr('stroke-width', 2)
                    .attr('class', 'node-shape');
            } else if (domain === 'file') {
                // Triangle
                g.append('polygon')
                    .attr('points', '0,-12 12,10 -12,10')
                    .attr('fill', this.colors.background)
                    .attr('stroke', d.metadata?.kind === 'directory' ? this.colors.file : '#9ca3af')
                    .attr('stroke-width', 2)
                    .attr('class', 'node-shape');
            }

            // Label
            g.append('text')
                .attr('dy', domain === 'resource' ? 35 : 28)
                .attr('text-anchor', 'middle')
                .attr('fill', '#e6edf3')
                .attr('font-size', '10px')
                .attr('font-family', 'JetBrains Mono, monospace')
                .attr('pointer-events', 'none')
                .text(d.label.length > 16 ? d.label.substring(0, 16) + '\u2026' : d.label);

            // Abbreviation inside node (resource only)
            if (domain === 'resource') {
                g.append('text')
                    .attr('dy', 4)
                    .attr('text-anchor', 'middle')
                    .attr('fill', '#fff')
                    .attr('font-size', '10px')
                    .attr('font-weight', 'bold')
                    .attr('font-family', 'JetBrains Mono, monospace')
                    .attr('pointer-events', 'none')
                    .text(d.label.substring(0, 4).toUpperCase());
            }
        });

        // Tick
        this.simulation.on('tick', () => {
            this.link
                .attr('x1', d => d.source.x)
                .attr('y1', d => d.source.y)
                .attr('x2', d => d.target.x)
                .attr('y2', d => d.target.y);
            this.node.attr('transform', d => `translate(${d.x},${d.y})`);
        });

        // Auto-fit after simulation settles (skip when preserving view, e.g. expand)
        this.simulation.on('end', opts.preserveView ? null : () => {
            this.fitToView();
        });

        // For large graphs, reduce iterations and fit early
        if (nodeCount > 200) {
            this.simulation.alphaDecay(0.05);
            if (!opts.preserveView) {
                if (this._fitTimer) clearTimeout(this._fitTimer);
                this._fitTimer = setTimeout(() => { this._fitTimer = null; this.fitToView(); }, 500);
            }
        }

        // Cancel any pending fitToView when preserving view
        if (opts.preserveView && this._fitTimer) {
            clearTimeout(this._fitTimer);
            this._fitTimer = null;
        }
    }

    getResourceColor(d) {
        const status = window.serviceStates?.[d.label] || 'stopped';
        return this.colors[status] || this.colors.stopped;
    }

    getEdgeWidth(confidence) {
        return confidence === 'definite' ? 2 : confidence === 'probable' ? 1.5 : 1;
    }

    getEdgeOpacity(confidence) {
        return confidence === 'definite' ? 0.6 : confidence === 'probable' ? 0.4 : 0.25;
    }

    getEdgeDash(confidence) {
        return confidence === 'definite' ? 'none' : confidence === 'probable' ? '4,2' : '2,4';
    }

    selectNode(d) {
        if (this.impactMode) {
            this.showImpact(d.id);
            return;
        }

        const wasSelected = this.selectedNode === d.id;
        this.selectedNode = wasSelected ? null : d.id;

        // Highlight selection
        this.node.selectAll('.node-shape')
            .attr('stroke-width', n => n.id === this.selectedNode ? 3 : 2)
            .attr('stroke', n => {
                if (n.id === this.selectedNode) return this.colors.active;
                if (n.domain === 'resource') return this.getResourceColor(n);
                return this.colors[n.domain] || this.colors.border;
            });

        // Dim unconnected nodes
        if (this.selectedNode) {
            const connected = new Set([this.selectedNode]);
            this.graphData.edges.forEach(e => {
                if (e.source === this.selectedNode || (e.source.id || e.source) === this.selectedNode) connected.add(e.target.id || e.target);
                if (e.target === this.selectedNode || (e.target.id || e.target) === this.selectedNode) connected.add(e.source.id || e.source);
            });
            this.node.style('opacity', n => connected.has(n.id) ? 1 : 0.3);
            this.link.style('opacity', e => {
                const src = e.source.id || e.source;
                const tgt = e.target.id || e.target;
                return (src === this.selectedNode || tgt === this.selectedNode) ? 0.8 : 0.1;
            });
        } else {
            this.node.style('opacity', 1);
            this.link.style('opacity', d => this.getEdgeOpacity(d.confidence));
        }

        // HTMX detail panel for resource nodes
        if (this.selectedNode && d.domain === 'resource') {
            htmx.ajax('GET', `/htmx/services/${d.label}`, {
                target: '#detail-panel',
                swap: 'innerHTML'
            });
            if (typeof openDetailPanel === 'function') openDetailPanel();
        }
    }

    async showImpact(nodeId) {
        try {
            const resp = await fetch(`/api/graph/impact/${encodeURIComponent(nodeId)}`);
            const impact = await resp.json();

            const affectedIds = new Set(impact.affected.map(a => a.id));
            affectedIds.add(impact.root);

            // Highlight impact
            this.node.selectAll('.node-shape')
                .attr('stroke-width', n => n.id === impact.root ? 3 : affectedIds.has(n.id) ? 2.5 : 2);
            this.node.style('opacity', n => affectedIds.has(n.id) ? 1 : 0.15);

            // Show impact summary
            this.showImpactSummary(impact);
        } catch (err) {
            console.error('Failed to get impact:', err);
        }
    }

    showImpactSummary(impact) {
        let existing = this.container.querySelector('.dtx-graph-impact-summary');
        if (existing) existing.remove();

        const div = document.createElement('div');
        div.className = 'dtx-graph-impact-summary';
        div.innerHTML = `
            <div class="text-sm font-mono">
                <div class="text-dtx-cyan font-bold mb-1">Impact: ${impact.affected.length} nodes affected</div>
                ${impact.affected.slice(0, 5).map(a =>
                    `<div class="text-gray-400 text-xs">${a.domain}: ${a.label} (d=${a.distance})</div>`
                ).join('')}
                ${impact.affected.length > 5 ? `<div class="text-gray-500 text-xs">...and ${impact.affected.length - 5} more</div>` : ''}
            </div>
        `;
        this.container.appendChild(div);
    }

    enableImpactMode(enabled) {
        this.impactMode = enabled;
        this.container.style.cursor = enabled ? 'crosshair' : 'default';
        const existing = this.container.querySelector('.dtx-graph-impact-summary');
        if (existing) existing.remove();
        if (enabled) {
            const hint = document.createElement('div');
            hint.className = 'dtx-graph-impact-summary';
            hint.innerHTML = '<div class="text-sm font-mono text-gray-400">Click a node to see its impact radius</div>';
            this.container.appendChild(hint);
        } else {
            this.node?.style('opacity', 1);
            this.node?.selectAll('.node-shape').attr('stroke-width', 2);
        }
    }

    showTooltip(event, d) {
        let info = `${d.domain}: ${d.label}`;
        if (d.domain === 'group' && d.metadata) {
            info = `${d.metadata.representative} (${d.metadata.count} ${d.metadata.child_domain}s)`;
            info += ' | Double-click to expand';
        } else if (d.metadata) {
            if (d.metadata.port) info += ` | :${d.metadata.port}`;
            if (d.metadata.kind) info += ` | ${d.metadata.kind}`;
            if (d.metadata.file) info += ` | ${d.metadata.file}:${d.metadata.line}`;
        }

        this.tooltip
            .style('opacity', 1)
            .html(info)
            .style('left', (event.offsetX + 15) + 'px')
            .style('top', (event.offsetY - 10) + 'px');
    }

    hideTooltip() {
        this.tooltip.style('opacity', 0);
    }

    updateNodeStatus(serviceName, status) {
        if (!this.graphData) return;

        const nodeId = `resource:${serviceName}`;
        this.node?.selectAll('.node-shape')
            .filter(d => d.id === nodeId)
            .transition()
            .duration(300)
            .attr('stroke', () => this.colors[status] || this.colors.stopped)
            .attr('filter', status === 'running' ? 'url(#glow)' : null);
    }

    filter(query) {
        if (!query || query.trim() === '') {
            this.node?.style('opacity', 1);
            this.link?.style('opacity', d => this.getEdgeOpacity(d.confidence));
            return;
        }
        const q = query.toLowerCase();
        this.node?.style('opacity', d => d.label.toLowerCase().includes(q) ? 1 : 0.2);
        this.link?.style('opacity', 0.1);
    }

    focusNode(nodeId) {
        if (!this.simulation) return;
        const node = this.simulation.nodes().find(n => n.id === nodeId);
        if (!node) return;

        // Select the node (triggers highlight + detail panel)
        this.selectNode(node);

        // Zoom/pan to center the node
        const width = this.container.clientWidth;
        const height = this.container.clientHeight || 500;
        const scale = 1.5;
        const transform = d3.zoomIdentity
            .translate(width / 2 - scale * node.x, height / 2 - scale * node.y)
            .scale(scale);
        this.svg.transition().duration(500).call(this.zoom.transform, transform);
    }

    fitToView() {
        const bounds = this.graphContainer.node().getBBox();
        if (bounds.width === 0 || bounds.height === 0) return;
        const detailPanel = document.getElementById('detail-panel-container');
        const panelOpen = detailPanel?.dataset?.open === 'true';
        const fullWidth = this.container.clientWidth - (panelOpen ? detailPanel.offsetWidth : 0);
        const fullHeight = this.container.clientHeight || 500;
        const maxScale = this.nodeCount <= 3 ? 1.5 : 2.5;
        const scale = Math.min(0.85 * Math.min(fullWidth / bounds.width, fullHeight / bounds.height), maxScale);
        const translate = [
            fullWidth / 2 - scale * (bounds.x + bounds.width / 2),
            fullHeight / 2 - scale * (bounds.y + bounds.height / 2)
        ];
        this.svg.transition().duration(500).call(
            this.zoom.transform,
            d3.zoomIdentity.translate(translate[0], translate[1]).scale(scale)
        );
    }

    zoomIn() {
        this.svg.transition().duration(300).call(this.zoom.scaleBy, 1.3);
    }

    zoomOut() {
        this.svg.transition().duration(300).call(this.zoom.scaleBy, 0.7);
    }

    resize() {
        const width = this.container.clientWidth;
        const height = this.container.clientHeight || 500;
        this.svg.attr('viewBox', [0, 0, width, height]);
        if (this.simulation) {
            this.simulation.force('center', d3.forceCenter(width / 2, height / 2));
            this.simulation.alpha(0.3).restart();
        }
    }

    drag() {
        const sim = () => this.simulation;
        return d3.drag()
            .on('start', function(event, d) {
                if (!event.active) sim().alphaTarget(0.3).restart();
                d.fx = d.x;
                d.fy = d.y;
            })
            .on('drag', function(event, d) {
                d.fx = event.x;
                d.fy = event.y;
            })
            .on('end', function(event, d) {
                if (!event.active) sim().alphaTarget(0);
                d.fx = null;
                d.fy = null;
            });
    }

    async expandNode(nodeId) {
        try {
            const resp = await fetch(`/api/graph/expand/${encodeURIComponent(nodeId)}`);
            if (!resp.ok) return;
            const partial = await resp.json();

            // Find the position of the group node being expanded
            let expandOrigin = null;
            if (this.simulation) {
                const groupNode = this.simulation.nodes().find(n => n.id === nodeId);
                if (groupNode) {
                    expandOrigin = { x: groupNode.x, y: groupNode.y };
                }
            }

            // Remove the group node
            delete this.graphData.nodes[nodeId];

            // Add children nodes
            for (const [id, node] of Object.entries(partial.nodes)) {
                this.graphData.nodes[id] = node;
            }

            // Remove edges referencing the group node
            this.graphData.edges = this.graphData.edges.filter(
                e => e.source !== nodeId && e.target !== nodeId
            );

            // Add new edges from the expanded graph
            if (partial.edges) {
                this.graphData.edges.push(...partial.edges);
            }

            this.render(this.graphData, { preserveView: true, expandOrigin });
        } catch (err) {
            console.error('Failed to expand node:', err);
        }
    }

    destroy() {
        if (this.simulation) this.simulation.stop();
        this.container.innerHTML = '';
    }
}

// Export for module usage
if (typeof module !== 'undefined' && module.exports) {
    module.exports = KnowledgeGraph;
}
