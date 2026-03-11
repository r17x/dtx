/**
 * ServiceGraph - D3.js force-directed graph for dtx services
 * Cyberpunk industrial aesthetic with real-time status updates
 */

class ServiceGraph {
    constructor(container, services, dependencies) {
        this.container = container;
        this.services = services;
        this.dependencies = dependencies;
        this.selectedNode = null;
        this.mode = 'all'; // 'all' or 'dependencies'

        // Design system colors
        this.colors = {
            running: '#22c55e',
            starting: '#f59e0b',
            stopped: '#6b7280',
            error: '#ec4899',
            active: '#22d3ee',
            ephemeral: '#a855f7',
            background: '#161b22',
            border: '#2d333b',
            link: '#2d333b'
        };

        // Graph dimensions
        this.width = container.clientWidth;
        this.height = container.clientHeight || 600;

        this.init();
    }

    init() {
        // Clear existing content
        this.container.innerHTML = '';

        // Create SVG with grid pattern background
        this.svg = d3.select(this.container)
            .append('svg')
            .attr('width', this.width)
            .attr('height', this.height)
            .attr('class', 'service-graph-svg');

        // Define grid pattern
        const defs = this.svg.append('defs');

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

        // Define glow filter for running services
        const filter = defs.append('filter')
            .attr('id', 'glow');

        filter.append('feGaussianBlur')
            .attr('stdDeviation', 3)
            .attr('result', 'coloredBlur');

        const feMerge = filter.append('feMerge');
        feMerge.append('feMergeNode').attr('in', 'coloredBlur');
        feMerge.append('feMergeNode').attr('in', 'SourceGraphic');

        // Background rect with grid
        this.svg.append('rect')
            .attr('width', this.width)
            .attr('height', this.height)
            .attr('fill', 'url(#grid)');

        // Create graph layers
        this.linkGroup = this.svg.append('g').attr('class', 'links');
        this.nodeGroup = this.svg.append('g').attr('class', 'nodes');

        // Build graph data
        this.buildGraph();

        // Create force simulation
        this.simulation = d3.forceSimulation(this.nodes)
            .force('link', d3.forceLink(this.links)
                .id(d => d.id)
                .distance(150))
            .force('charge', d3.forceManyBody()
                .strength(-300))
            .force('center', d3.forceCenter(this.width / 2, this.height / 2))
            .force('collision', d3.forceCollide().radius(50));

        this.render();
    }

    buildGraph() {
        // Build nodes from services
        this.nodes = this.services.map(service => ({
            id: service.id,
            name: service.name,
            status: service.status || 'stopped',
            port: service.port,
            package: service.package,
            enabled: service.enabled,
            abbreviation: this.getAbbreviation(service.name)
        }));

        // Build links from dependencies
        this.links = [];
        this.dependencies.forEach(dep => {
            if (dep.depends_on && Array.isArray(dep.depends_on)) {
                dep.depends_on.forEach(targetId => {
                    this.links.push({
                        source: dep.service_id,
                        target: targetId
                    });
                });
            }
        });
    }

    getAbbreviation(name) {
        // Create 2-3 letter abbreviation from service name
        const parts = name.split(/[-_]/);
        if (parts.length > 1) {
            return parts.map(p => p[0]).join('').substring(0, 3).toUpperCase();
        }
        return name.substring(0, 3).toUpperCase();
    }

    render() {
        // Render links
        this.link = this.linkGroup.selectAll('line')
            .data(this.links)
            .enter()
            .append('line')
            .attr('stroke', this.colors.link)
            .attr('stroke-width', 2)
            .attr('stroke-opacity', 0.6)
            .attr('marker-end', 'url(#arrow)');

        // Define arrow marker
        this.svg.select('defs')
            .append('marker')
            .attr('id', 'arrow')
            .attr('viewBox', '0 -5 10 10')
            .attr('refX', 25)
            .attr('refY', 0)
            .attr('markerWidth', 6)
            .attr('markerHeight', 6)
            .attr('orient', 'auto')
            .append('path')
            .attr('d', 'M0,-5L10,0L0,5')
            .attr('fill', this.colors.link);

        // Create node groups
        this.node = this.nodeGroup.selectAll('g')
            .data(this.nodes)
            .enter()
            .append('g')
            .attr('class', 'node')
            .call(this.drag());

        // Render node circles
        this.nodeCircle = this.node.append('circle')
            .attr('r', 20)
            .attr('fill', d => this.getStatusColor(d.status))
            .attr('stroke', d => d.status === 'running' ? this.colors.active : this.colors.border)
            .attr('stroke-width', 2)
            .attr('filter', d => d.status === 'running' ? 'url(#glow)' : null)
            .on('click', (event, d) => this.handleNodeClick(event, d));

        // Add abbreviation text
        this.node.append('text')
            .attr('class', 'node-abbr')
            .attr('text-anchor', 'middle')
            .attr('dy', '.35em')
            .attr('fill', '#fff')
            .attr('font-size', '12px')
            .attr('font-weight', 'bold')
            .attr('pointer-events', 'none')
            .text(d => d.abbreviation);

        // Add full name label
        this.node.append('text')
            .attr('class', 'node-label')
            .attr('text-anchor', 'middle')
            .attr('dy', 35)
            .attr('fill', '#fff')
            .attr('font-size', '10px')
            .attr('opacity', 0.7)
            .attr('pointer-events', 'none')
            .text(d => d.name);

        // Add status indicator
        this.node.append('circle')
            .attr('class', 'status-indicator')
            .attr('r', 4)
            .attr('cx', 14)
            .attr('cy', -14)
            .attr('fill', d => this.getStatusColor(d.status))
            .attr('stroke', this.colors.background)
            .attr('stroke-width', 1);

        // Update positions on simulation tick
        this.simulation.on('tick', () => {
            this.link
                .attr('x1', d => d.source.x)
                .attr('y1', d => d.source.y)
                .attr('x2', d => d.target.x)
                .attr('y2', d => d.target.y);

            this.node
                .attr('transform', d => `translate(${d.x},${d.y})`);
        });
    }

    getStatusColor(status) {
        const colorMap = {
            running: this.colors.running,
            starting: this.colors.starting,
            stopped: this.colors.stopped,
            error: this.colors.error
        };
        return colorMap[status] || this.colors.stopped;
    }

    handleNodeClick(event, d) {
        event.stopPropagation();

        // Update selection
        if (this.selectedNode === d.id) {
            this.selectedNode = null;
        } else {
            this.selectedNode = d.id;
        }

        // Update visual selection
        this.nodeCircle
            .attr('stroke', node => {
                if (node.id === this.selectedNode) {
                    return this.colors.active;
                }
                return node.status === 'running' ? this.colors.active : this.colors.border;
            })
            .attr('stroke-width', node => node.id === this.selectedNode ? 3 : 2);

        // Trigger HTMX to load detail panel
        if (this.selectedNode) {
            const detailPanel = document.querySelector('#detail-panel');
            if (detailPanel) {
                htmx.ajax('GET', `/htmx/services/${d.id}`, {
                    target: '#detail-panel',
                    swap: 'innerHTML'
                });
                if (typeof openDetailPanel === 'function') {
                    openDetailPanel();
                }
            }
        }
    }

    drag() {
        function dragstarted(event, d) {
            if (!event.active) this.simulation.alphaTarget(0.3).restart();
            d.fx = d.x;
            d.fy = d.y;
        }

        function dragged(event, d) {
            d.fx = event.x;
            d.fy = event.y;
        }

        function dragended(event, d) {
            if (!event.active) this.simulation.alphaTarget(0);
            d.fx = null;
            d.fy = null;
        }

        return d3.drag()
            .on('start', dragstarted.bind(this))
            .on('drag', dragged.bind(this))
            .on('end', dragended.bind(this));
    }

    /**
     * Update service status in real-time
     * @param {string} serviceId - Service ID
     * @param {string} status - New status (running, stopped, starting, error)
     */
    updateStatus(serviceId, status) {
        // Find and update node data
        const node = this.nodes.find(n => n.id === serviceId);
        if (!node) return;

        node.status = status;

        // Update visuals
        this.nodeCircle
            .filter(d => d.id === serviceId)
            .transition()
            .duration(300)
            .attr('fill', this.getStatusColor(status))
            .attr('stroke', status === 'running' ? this.colors.active : this.colors.border)
            .attr('filter', status === 'running' ? 'url(#glow)' : null);

        this.node.selectAll('.status-indicator')
            .filter(d => d.id === serviceId)
            .transition()
            .duration(300)
            .attr('fill', this.getStatusColor(status));
    }

    /**
     * Filter services by search query
     * @param {string} query - Search query
     */
    filter(query) {
        if (!query || query.trim() === '') {
            // Show all nodes
            this.node.style('opacity', 1);
            this.link.style('opacity', 0.6);
            return;
        }

        const lowerQuery = query.toLowerCase();

        // Filter nodes
        this.node.style('opacity', d => {
            const matches = d.name.toLowerCase().includes(lowerQuery) ||
                          (d.package && d.package.toLowerCase().includes(lowerQuery));
            return matches ? 1 : 0.2;
        });

        // Filter links - only show if both nodes match
        this.link.style('opacity', d => {
            const sourceMatches = d.source.name.toLowerCase().includes(lowerQuery);
            const targetMatches = d.target.name.toLowerCase().includes(lowerQuery);
            return (sourceMatches && targetMatches) ? 0.6 : 0.1;
        });
    }

    /**
     * Set graph display mode
     * @param {string} mode - 'all' or 'dependencies'
     */
    setMode(mode) {
        this.mode = mode;

        if (mode === 'dependencies') {
            // Only show nodes with dependencies
            const connectedNodeIds = new Set();
            this.links.forEach(link => {
                connectedNodeIds.add(link.source.id || link.source);
                connectedNodeIds.add(link.target.id || link.target);
            });

            this.node.style('opacity', d =>
                connectedNodeIds.has(d.id) ? 1 : 0.2
            );
            this.link.style('opacity', 0.6);
        } else {
            // Show all nodes
            this.node.style('opacity', 1);
            this.link.style('opacity', 0.6);
        }
    }

    /**
     * Resize graph to fit container
     */
    resize() {
        this.width = this.container.clientWidth;
        this.height = this.container.clientHeight || 600;

        this.svg
            .attr('width', this.width)
            .attr('height', this.height);

        this.simulation
            .force('center', d3.forceCenter(this.width / 2, this.height / 2))
            .alpha(0.3)
            .restart();
    }

    /**
     * Destroy graph and cleanup
     */
    destroy() {
        this.simulation.stop();
        this.container.innerHTML = '';
    }
}

// Export for module usage
if (typeof module !== 'undefined' && module.exports) {
    module.exports = ServiceGraph;
}
